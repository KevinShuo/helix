mod draft;
mod tasks;
mod view;

use crate::{
    compositor::{Component, Context, Event, EventResult},
    handlers::ai_commit,
    ui::{Prompt, PromptEvent},
};
use anyhow::{ensure, Result};
use draft::Draft;
use helix_core::Position;
use helix_vcs::git_tools::{CommitSnapshot, GitRepository};
use helix_view::{
    ai_commit::CommitLanguage,
    graphics::{CursorKind, Rect},
    input::{KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    Editor,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::task::JoinHandle;
use tui::buffer::Buffer;

const ID: &str = "ai-commit";
static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    Idle,
    Reading,
    Ai,
    Writing,
}

#[derive(Clone, Copy)]
struct Row {
    id: usize,
    staged: bool,
}

pub(crate) struct CommitWorkspace {
    instance: u64,
    serial: u64,
    root: PathBuf,
    repo: Option<GitRepository>,
    snapshot: Option<CommitSnapshot>,
    selected: BTreeSet<usize>,
    reasons: BTreeMap<usize, String>,
    row: usize,
    diff_scroll: usize,
    diff_focus: bool,
    draft: Draft,
    editing: bool,
    intent: String,
    language: CommitLanguage,
    busy: Busy,
    task: Option<JoinHandle<()>>,
    message: String,
    focused: bool,
    fullscreen: bool,
    area: Rect,
    files_area: Rect,
    diff_area: Rect,
    message_area: Rect,
}

impl Drop for CommitWorkspace {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        // Confirmed Git mutations belong to Jobs and finish independently.
    }
}

pub(crate) fn open(
    compositor: &mut crate::compositor::Compositor,
    editor: &mut Editor,
    intent: String,
) -> Result<()> {
    let root = editor
        .git_context_path()
        .ok_or_else(|| anyhow::anyhow!("当前缓冲区没有 Git 路径"))?;
    ensure!(
        super::trusted(editor, &root),
        "请先使用 :trust 信任当前 Git 工作区"
    );
    ensure!(
        editor.macro_recording.is_none() && editor.macro_replaying.is_empty(),
        "宏录制或重放期间不支持 AI 提交"
    );
    ensure!(
        editor.active_merge().is_none(),
        "请先完成或退出当前冲突处理"
    );
    if let Some(panel) = compositor.find::<CommitWorkspace>() {
        let same = panel
            .repo
            .as_ref()
            .is_some_and(|r| in_current_worktree(r, &root))
            || panel.root == root;
        if same {
            panel.focused = true;
            return Ok(());
        }
        ensure!(
            panel.busy != Busy::Writing,
            "Git 写操作仍在运行，请等待完成"
        );
    }
    cancel(compositor, editor);
    compositor.remove(ID);
    if let Some(panel) = compositor.find::<super::GitPanel>() {
        panel.focused = false;
        panel.suspended = true;
    }
    let mut panel = CommitWorkspace {
        instance: NEXT.fetch_add(1, Ordering::Relaxed),
        serial: 0,
        root,
        repo: None,
        snapshot: None,
        selected: BTreeSet::new(),
        reasons: BTreeMap::new(),
        row: 0,
        diff_scroll: 0,
        diff_focus: false,
        draft: Draft::default(),
        editing: false,
        intent,
        language: editor.config().ai_commit.language,
        busy: Busy::Idle,
        task: None,
        message: String::new(),
        focused: true,
        fullscreen: false,
        area: Rect::default(),
        files_area: Rect::default(),
        diff_area: Rect::default(),
        message_area: Rect::default(),
    };
    panel.refresh(editor, true);
    compositor.push(Box::new(panel));
    Ok(())
}

pub(crate) fn cancel(compositor: &mut crate::compositor::Compositor, editor: &mut Editor) {
    if let Some(panel) = compositor.find::<CommitWorkspace>() {
        panel.cancel_request(editor);
    }
}

impl CommitWorkspace {
    fn cancel_request(&mut self, editor: &mut Editor) {
        if matches!(self.busy, Busy::Ai | Busy::Reading) {
            if let Some(task) = self.task.take() {
                task.abort();
            }
            self.serial += 1;
            if self.busy == Busy::Ai {
                editor.ai_commit_running = false;
            }
            self.busy = Busy::Idle;
            self.message = "已取消分析；暂存内容和提交草稿保留".into();
        }
    }
    fn rows(&self) -> Vec<Row> {
        let Some(s) = &self.snapshot else {
            return vec![];
        };
        s.files
            .iter()
            .filter(|f| f.staged)
            .map(|f| Row {
                id: f.id,
                staged: true,
            })
            .chain(
                s.files
                    .iter()
                    .filter(|f| f.unstaged || f.untracked)
                    .map(|f| Row {
                        id: f.id,
                        staged: false,
                    }),
            )
            .collect()
    }
    fn active(&self) -> Option<Row> {
        self.rows().get(self.row).copied()
    }
    fn has_staged(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|s| s.files.iter().any(|f| f.staged))
    }
    fn toggle(&mut self) {
        if let Some(Row { id, staged: false }) = self.active() {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
            self.message = "候选已调整，尚未执行 add。s 暂存勾选的完整磁盘版本".into();
        }
    }
    fn dirty(&self, editor: &Editor, ids: Option<&[usize]>) -> bool {
        let (Some(repo), Some(snapshot)) = (&self.repo, &self.snapshot) else {
            return false;
        };
        snapshot
            .files
            .iter()
            .filter(|f| ids.map_or(f.staged, |ids| ids.contains(&f.id)))
            .any(|file| {
                std::iter::once(&file.path)
                    .chain(file.old_path.iter())
                    .any(|p| {
                        let full = repo.root().join(p);
                        editor.documents().any(|doc| {
                            doc.is_modified()
                                && doc.path().is_some_and(|path| same_path(path, &full))
                        })
                    })
            })
    }
    fn intent_prompt(&self, editor: &Editor) -> Prompt {
        let instance = self.instance;
        Prompt::new(
            "提交意图: ".into(),
            None,
            |_, _| Vec::new(),
            move |_, input, event| {
                if event != PromptEvent::Validate {
                    return;
                }
                let input = input.to_owned();
                crate::job::dispatch_blocking(move |editor, c| {
                    if let Some(panel) = c.find::<Self>().filter(|p| p.instance == instance) {
                        panel.cancel_request(editor);
                        panel.intent = input;
                        panel.message = "意图已更新，a 重新推荐 / g 重新生成提交信息".into();
                    }
                });
            },
        )
        .with_line(self.intent.clone(), editor)
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

fn in_current_worktree(repo: &GitRepository, directory: &Path) -> bool {
    if !repo.contains_path(directory).unwrap_or(false) {
        return false;
    }
    let Ok(mut directory) = std::fs::canonicalize(directory) else {
        return false;
    };
    let Ok(root) = std::fs::canonicalize(repo.root()) else {
        return false;
    };
    loop {
        if directory == root {
            return true;
        }
        // A nested repository/submodule/worktree has its own index and HEAD.
        // Being inside the old root is insufficient to reuse its draft.
        if directory.join(".git").exists() || !directory.pop() {
            return false;
        }
    }
}

impl Component for CommitWorkspace {
    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
    fn takes_keyboard_input(&self) -> bool {
        self.focused
    }
    fn editor_area(&self, area: Rect) -> Rect {
        if self.fullscreen || area.height < 16 {
            area
        } else {
            area.clip_bottom(self.dock(area).height)
        }
    }
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if let Event::Mouse(mouse) = event {
            if contains(self.area, mouse.column, mouse.row) {
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.focused = true;
                        if contains(self.files_area, mouse.column, mouse.row) {
                            let offset = self.row.saturating_sub(
                                self.files_area.height.saturating_sub(2) as usize / 2,
                            );
                            self.row = (offset
                                + mouse.row.saturating_sub(self.files_area.y + 1) as usize)
                                .min(self.rows().len().saturating_sub(1));
                            self.diff_focus = false;
                            self.diff_scroll = 0;
                        } else if contains(self.diff_area, mouse.column, mouse.row) {
                            self.diff_focus = true;
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        self.diff_scroll = self.diff_scroll.saturating_add(3)
                    }
                    MouseEventKind::ScrollUp => {
                        self.diff_scroll = self.diff_scroll.saturating_sub(3)
                    }
                    _ => {}
                }
                return EventResult::Consumed(None);
            }
        }
        if let Event::Paste(text) = event {
            if !self.focused {
                return EventResult::Ignored(None);
            }
            if self.editing && self.busy != Busy::Writing {
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                let text = text
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\n')
                    .collect::<String>();
                if self.draft.text.len() + text.len() <= 16_384 {
                    self.draft.insert(&text);
                } else {
                    self.message = "粘贴内容超过提交信息长度限制".into();
                }
            }
            return EventResult::Consumed(None);
        }
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        if !self.focused {
            return EventResult::Ignored(None);
        }
        if self.busy == Busy::Writing
            && !matches!(
                key.code,
                KeyCode::Esc
                    | KeyCode::Char('q')
                    | KeyCode::F(11)
                    | KeyCode::Tab
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            )
        {
            self.message = "Git 写操作正在运行，请等待；可继续浏览 diff 或关闭窗口".into();
            return EventResult::Consumed(None);
        }
        if self.editing {
            match key.code {
                KeyCode::Esc => self.editing = false,
                KeyCode::Char('s') if key.modifiers == KeyModifiers::CONTROL => {
                    self.editing = false
                }
                KeyCode::Char(c)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    if self.draft.text.len() < 65536 && !c.is_control() {
                        self.draft.insert(&c.to_string());
                    }
                }
                KeyCode::Enter => {
                    if self.draft.text.len() < 65536 {
                        self.draft.insert("\n");
                    }
                }
                KeyCode::Backspace => self.draft.backspace(),
                KeyCode::Delete => self.draft.delete(),
                KeyCode::Left => self.draft.left(),
                KeyCode::Right => self.draft.right(),
                KeyCode::Up => self.draft.vertical(false),
                KeyCode::Down => self.draft.vertical(true),
                KeyCode::Home => {
                    self.draft.cursor = self.draft.text[..self.draft.cursor]
                        .rfind('\n')
                        .map_or(0, |i| i + 1)
                }
                KeyCode::End => {
                    self.draft.cursor = self.draft.text[self.draft.cursor..]
                        .find('\n')
                        .map_or(self.draft.text.len(), |i| self.draft.cursor + i)
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('w') {
            self.focused = false;
            return EventResult::Consumed(None);
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            self.cancel_request(cx.editor);
            return EventResult::Consumed(None);
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return EventResult::Consumed(None);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.cancel_request(cx.editor);
                return EventResult::Consumed(Some(Box::new(|c, _| {
                    c.remove(ID);
                    if let Some(panel) = c.find::<super::GitPanel>() {
                        panel.suspended = false;
                        panel.focused = true;
                    }
                })));
            }
            KeyCode::F(11) | KeyCode::Char('z') => self.fullscreen = !self.fullscreen,
            KeyCode::Char('e') => {
                self.cancel_request(cx.editor);
                self.editing = true;
                self.draft.cursor = self.draft.text.len();
            }
            KeyCode::Char('l') => {
                self.cancel_request(cx.editor);
                self.language = match self.language {
                    CommitLanguage::Chinese => CommitLanguage::English,
                    CommitLanguage::English => CommitLanguage::Chinese,
                };
                self.message = "语言已切换；草稿保留。g 重新生成后替换当前文本".into();
            }
            KeyCode::Char('i') => {
                let prompt = self.intent_prompt(cx.editor);
                return EventResult::Consumed(Some(Box::new(move |c, _| c.push(Box::new(prompt)))));
            }
            KeyCode::Char('r') | KeyCode::F(5) if self.busy != Busy::Writing => {
                self.cancel_request(cx.editor);
                self.refresh(cx.editor, false);
            }
            KeyCode::Char('a') => self.start_ai(cx.editor, true),
            KeyCode::Char('g') => self.start_ai(cx.editor, false),
            KeyCode::Char('d') => self.show_patch(cx.editor),
            KeyCode::Char('s') => self.mutate(cx, 0),
            KeyCode::Char('u') => self.mutate(cx, 1),
            KeyCode::Char('C') => self.mutate(cx, 2),
            KeyCode::Char('v') => {
                if ai_commit::validate_message(&self.draft.text).is_ok() && self.has_staged() {
                    self.draft.keep_manual();
                    self.message = "已明确保留当前文本；请核对真实暂存区，C 提交".into();
                } else {
                    self.message = "请先 e 编辑合法提交信息，并确保暂存区非空".into();
                }
            }
            KeyCode::Char(' ') => {
                self.cancel_request(cx.editor);
                self.toggle();
            }
            KeyCode::Tab => self.diff_focus = !self.diff_focus,
            KeyCode::Enter => self.diff_focus = true,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.diff_focus {
                    self.diff_scroll = self.diff_scroll.saturating_add(1)
                } else {
                    self.row = (self.row + 1).min(self.rows().len().saturating_sub(1));
                    self.diff_scroll = 0;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.diff_focus {
                    self.diff_scroll = self.diff_scroll.saturating_sub(1)
                } else {
                    self.row = self.row.saturating_sub(1);
                    self.diff_scroll = 0;
                }
            }
            KeyCode::PageDown => self.diff_scroll = self.diff_scroll.saturating_add(10),
            KeyCode::PageUp => self.diff_scroll = self.diff_scroll.saturating_sub(10),
            _ => {}
        }
        EventResult::Consumed(None)
    }
    fn render(&mut self, area: Rect, frame: &mut Buffer, cx: &mut Context) {
        self.draw(area, frame, cx.editor);
    }
    fn cursor(&self, _area: Rect, _editor: &Editor) -> (Option<Position>, CursorKind) {
        (None, CursorKind::Hidden)
    }
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}
