//! Native result Document with render-only immutable source panes.
pub(super) mod render;
mod writes;
use crate::{
    commands,
    compositor::{Component, Compositor, Context, Event, EventResult},
};
use helix_core::{Selection, Transaction};
use helix_vcs::merge::{ConflictChoice, MergeLineMaps, MergeMode, MergeModel};
use helix_view::{
    graphics::Rect,
    input::{KeyCode, MouseButton, MouseEventKind},
    merge::{MergeSessionId, MergeWriteStatus},
    Editor,
};
use std::path::PathBuf;
pub(super) use writes::reload_clean;

const ID: &str = "git-merge-editor";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MergeUiAction {
    Next,
    Previous,
    UseCurrent,
    UseIncoming,
    KeepBoth,
    ShowBase,
    MarkReviewed,
    Apply,
    Files,
    AcceptCurrentFile,
    AcceptIncomingFile,
    Rebuild,
    RetryStage,
    Refresh,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preview {
    Result,
    Current,
    Incoming,
    Base,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct MergeLayout {
    pub current: Rect,
    pub result: Rect,
    pub incoming: Rect,
    pub header: Rect,
    pub footer: Rect,
    pub wide: bool,
}
impl MergeLayout {
    pub fn for_area(area: Rect) -> Self {
        let wide = area.width >= 120 && area.height >= 18;
        let content = area.clip_bottom(area.height.min(1));
        let header = content.with_height(content.height.min(2));
        let rest = content.clip_top(header.height);
        let body = rest.clip_bottom(rest.height.min(1));
        let footer = Rect::new(
            area.x,
            body.bottom(),
            area.width,
            content.bottom().saturating_sub(body.bottom()),
        );
        let width = if wide { body.width * 3 / 10 } else { 0 };
        Self {
            current: body.with_width(width),
            result: body.clip_left(width).clip_right(width),
            incoming: Rect::new(body.right() - width, body.y, width, body.height),
            header,
            footer,
            wide,
        }
    }
}
pub(crate) struct MergeEditor {
    instance: u64,
    session: MergeSessionId,
    selected: usize,
    preview: Preview,
    scroll: [usize; 3],
    maps: Option<(i32, MergeLineMaps)>,
    confirmation: Option<MergeUiAction>,
    last_layout: MergeLayout,
}
pub(super) fn install(compositor: &mut Compositor, session: MergeSessionId) {
    compositor.remove(ID);
    compositor.push(Box::new(MergeEditor {
        instance: super::NEXT_INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        session,
        selected: 0,
        preview: Preview::Result,
        scroll: [0; 3],
        maps: None,
        confirmation: None,
        last_layout: MergeLayout::for_area(Rect::default()),
    }));
}
#[allow(dead_code)] // Path-based entry point shared with conflict discovery callers.
pub(crate) fn open_merge(cx: &mut commands::Context, path: PathBuf) {
    if !super::trusted(cx.editor, &super::conflicts::root(cx.editor)) {
        cx.editor
            .set_error("Git tools require a trusted workspace (:trust)");
        return;
    }
    super::conflicts::open(cx);
    cx.callback.push(Box::new(move |compositor, _| {
        if let Some(list) = compositor.find::<super::conflicts::ConflictList>() {
            list.request_file(path, false, None);
        }
    }));
}
pub(crate) fn merge_action(cx: &mut commands::Context, action: MergeUiAction) {
    if cx.editor.active_merge().is_none() {
        cx.editor
            .set_error("Open a conflict file first (Space v r)");
        return;
    }
    cx.callback.push(Box::new(move |compositor, cx| {
        let Some(merge) = compositor.find::<MergeEditor>() else {
            return;
        };
        let event = merge.action(action, cx, false);
        if let EventResult::Consumed(Some(callback)) = event {
            callback(compositor, cx);
        }
    }));
}
impl MergeEditor {
    fn active(&self, editor: &Editor) -> bool {
        editor.active_merge().is_some()
    }
    fn sync_session(&mut self, editor: &Editor) -> bool {
        let Some(session) = editor.active_merge() else {
            return false;
        };
        if session != self.session {
            self.instance = super::NEXT_INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.session = session;
            self.selected = 0;
            self.preview = Preview::Result;
            self.scroll = [0; 3];
            self.maps = None;
            self.confirmation = None;
        }
        true
    }
    fn source_key(&mut self, code: KeyCode) {
        source_key(&mut self.preview, &mut self.scroll, code);
    }
    fn source_index(&self) -> usize {
        match self.preview {
            Preview::Base => 0,
            Preview::Current => 1,
            _ => 2,
        }
    }
    fn action(&mut self, action: MergeUiAction, cx: &mut Context, confirmed: bool) -> EventResult {
        use MergeUiAction::*;
        if !self.sync_session(cx.editor) {
            return EventResult::Ignored(None);
        }
        let (_, doc) = helix_view::current_ref!(cx.editor);
        if doc.merge().unwrap().busy && !matches!(action, Files | ShowBase | Next | Previous) {
            cx.editor.set_error("Merge result is being applied");
            return EventResult::Consumed(None);
        }
        if matches!(doc.merge().unwrap().write_status, MergeWriteStatus::Staged)
            && matches!(
                action,
                UseCurrent
                    | UseIncoming
                    | KeepBoth
                    | MarkReviewed
                    | AcceptCurrentFile
                    | AcceptIncomingFile
                    | Rebuild
            )
        {
            cx.editor
                .set_error("Staged result is read-only; return to Files to continue the merge");
            return EventResult::Consumed(None);
        }
        if matches!(
            action,
            AcceptCurrentFile | AcceptIncomingFile | Rebuild | Refresh
        ) && !confirmed
        {
            self.confirmation = Some(action);
            return EventResult::Consumed(None);
        }
        if matches!(action, Rebuild | Refresh)
            && !super::trusted(cx.editor, &super::conflicts::root(cx.editor))
        {
            cx.editor
                .set_error("Git tools require a trusted workspace (:trust)");
            return EventResult::Consumed(None);
        }
        match action {
            Files => {
                let path = super::conflicts::root(cx.editor);
                if let Err(error) = cx
                    .editor
                    .leave_merge(helix_view::merge::MergeLeave::KeepDraft)
                {
                    cx.editor.set_error(error.to_string());
                }
                return EventResult::Consumed(Some(Box::new(move |compositor, cx| {
                    super::conflicts::present(compositor, cx.editor, path)
                })));
            }
            ShowBase => {
                self.preview = if self.preview == Preview::Base {
                    Preview::Result
                } else {
                    Preview::Base
                }
            }
            Apply | RetryStage => writes::apply(cx, action == RetryStage),
            Refresh => {
                let path = doc.merge().unwrap().data.file.path.clone();
                let root = doc.merge().unwrap().data.version.root().to_owned();
                let expected = (self.session, doc.version());
                return EventResult::Consumed(Some(Box::new(move |compositor, cx| {
                    super::conflicts::refresh_draft(compositor, cx.editor, root, path, expected);
                })));
            }
            Rebuild => {
                // Read-only generation binds to both the active session and document version.
                let id = doc.id();
                let version = doc.version();
                let data = doc.merge().unwrap().data.clone();
                let session = self.session;
                let instance = self.instance;
                cx.jobs.callback(async move {
                    let repository = helix_vcs::git_tools::GitRepository::open(
                        data.version.root().to_owned(),
                        true,
                    )
                    .await?;
                    let candidate = repository.merge_candidate(&data).await?;
                    let model = candidate.rebuild()?;
                    Ok(crate::job::Callback::EditorCompositor(Box::new(
                        move |editor, compositor| {
                            if !compositor
                                .find::<MergeEditor>()
                                .is_some_and(|merge| merge.instance == instance)
                            {
                                return;
                            }
                            if editor.active_merge() != Some(session)
                                || editor.documents.get(&id).is_none_or(|d| {
                                    d.version() != version || d.merge().is_some_and(|m| m.busy)
                                })
                            {
                                return;
                            }
                            replace_model(editor, model);
                        },
                    )))
                });
            }
            AcceptCurrentFile | AcceptIncomingFile => {
                let model = &doc.merge().unwrap().model;
                let text = if action == AcceptCurrentFile {
                    model.sources().current()
                } else {
                    model.sources().incoming()
                }
                .clone();
                let model = MergeModel::preserve_working(text, model.sources().clone());
                replace_model(cx.editor, model);
            }
            Next | Previous => {
                self.scroll = [0; 3];
                let len = doc.merge().unwrap().model.blocks().len();
                if len > 0 {
                    self.selected = if action == Next {
                        (self.selected + 1) % len
                    } else {
                        (self.selected + len - 1) % len
                    };
                }
                let (view, doc) = helix_view::current!(cx.editor);
                if let Some(block) = doc.merge().unwrap().model.blocks().get(self.selected) {
                    let position = block.result.start.min(doc.text().len_chars());
                    doc.set_selection(view.id, Selection::point(position));
                    view.ensure_cursor_in_view(doc, 3);
                }
            }
            MarkReviewed => {
                let (_, doc) = helix_view::current!(cx.editor);
                let model = &mut doc.merge_mut().unwrap().model;
                model.mark_reviewed(if model.mode() == MergeMode::WholeFileReview {
                    None
                } else {
                    Some(self.selected)
                });
            }
            UseCurrent | UseIncoming | KeepBoth => {
                let (view, doc) = helix_view::current!(cx.editor);
                doc.append_changes_to_history(view);
                let choice = match action {
                    UseCurrent => ConflictChoice::Current,
                    UseIncoming => ConflictChoice::Incoming,
                    _ => ConflictChoice::Both,
                };
                match doc
                    .merge()
                    .unwrap()
                    .model
                    .choice(self.selected, choice, doc.text().slice(..))
                {
                    Ok(tx) => {
                        if doc.apply(&tx, view.id) {
                            doc.merge_mut()
                                .unwrap()
                                .model
                                .mark_reviewed(Some(self.selected));
                            doc.append_changes_to_history(view);
                        }
                    }
                    Err(error) => cx.editor.set_error(error.to_string()),
                }
            }
        }
        EventResult::Consumed(None)
    }
}
fn source_key(preview: &mut Preview, scroll: &mut [usize; 3], code: KeyCode) {
    let index = match preview {
        Preview::Base => 0,
        Preview::Current => 1,
        _ => 2,
    };
    match code {
        KeyCode::Char('2') | KeyCode::Esc => *preview = Preview::Result,
        KeyCode::Char('1') => *preview = Preview::Current,
        KeyCode::Char('3') => *preview = Preview::Incoming,
        KeyCode::Char('4') => *preview = Preview::Base,
        KeyCode::Char('j') | KeyCode::Down => scroll[index] = scroll[index].saturating_add(1),
        KeyCode::Char('k') | KeyCode::Up => scroll[index] = scroll[index].saturating_sub(1),
        _ => {}
    }
}
fn replace_model(editor: &mut Editor, model: MergeModel) {
    let (view, doc) = helix_view::current!(editor);
    doc.append_changes_to_history(view);
    let tx = Transaction::change(
        doc.text(),
        [(
            0,
            doc.text().len_chars(),
            Some(model.initial_result().to_string().into()),
        )]
        .into_iter(),
    );
    if doc.apply(&tx, view.id) {
        doc.merge_mut().unwrap().model = model;
        doc.append_changes_to_history(view);
    }
}
impl Component for MergeEditor {
    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }
    fn editor_area_for(&self, area: Rect, editor: &Editor) -> Rect {
        if self.active(editor) {
            let result = MergeLayout::for_area(area).result;
            result.with_height(
                result
                    .height
                    .saturating_add(1)
                    .min(area.bottom().saturating_sub(result.y)),
            )
        } else {
            area
        }
    }
    fn render(&mut self, area: Rect, frame: &mut tui::buffer::Buffer, cx: &mut Context) {
        if !self.sync_session(cx.editor) {
            return;
        }
        self.last_layout = MergeLayout::for_area(area);
        self.draw(frame, cx);
        if let Some(mut info) = cx.editor.autoinfo.take() {
            info.render(area, frame, cx);
            cx.editor.autoinfo = Some(info);
        }
    }
    fn cursor(
        &self,
        area: Rect,
        editor: &Editor,
    ) -> (
        Option<helix_core::Position>,
        helix_view::graphics::CursorKind,
    ) {
        (
            (self.active(editor)
                && (self.preview != Preview::Result || self.confirmation.is_some()))
            .then_some(helix_core::Position::new(area.y as usize, area.x as usize)),
            helix_view::graphics::CursorKind::Hidden,
        )
    }
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if !self.sync_session(cx.editor) {
            return EventResult::Ignored(None);
        }
        if matches!(event, Event::FocusGained | Event::FocusLost) {
            return EventResult::Ignored(None);
        }
        if let Some(action) = self.confirmation {
            if let Event::Key(key) = event {
                if key.code == KeyCode::Char('y') {
                    self.confirmation = None;
                    return self.action(action, cx, true);
                }
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('n')) {
                    self.confirmation = None;
                }
            }
            return EventResult::Consumed(None);
        }
        if let Event::Mouse(mouse) = event {
            let layout = self.last_layout;
            let inside = |r: Rect| {
                mouse.column >= r.x
                    && mouse.column < r.right()
                    && mouse.row >= r.y
                    && mouse.row < r.bottom()
            };
            if inside(layout.header)
                && mouse.row == layout.header.y + 1
                && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            {
                self.preview = if layout.wide {
                    if mouse.column < layout.result.x {
                        Preview::Current
                    } else if mouse.column >= layout.incoming.x {
                        Preview::Incoming
                    } else {
                        Preview::Result
                    }
                } else {
                    match (mouse.column - layout.header.x) / 18 {
                        0 => Preview::Current,
                        1 => Preview::Result,
                        2 => Preview::Incoming,
                        _ => Preview::Base,
                    }
                };
                return EventResult::Consumed(None);
            }
            if inside(layout.footer) {
                if self.preview == Preview::Result
                    && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                {
                    if let Some(action) = render::button_at(layout.footer, mouse.column) {
                        return self.action(action, cx, false);
                    }
                }
                return EventResult::Consumed(None);
            }
            let side = if inside(layout.current) {
                Some(1)
            } else if inside(layout.incoming) {
                Some(2)
            } else if self.preview != Preview::Result {
                Some(self.source_index())
            } else {
                None
            };
            if layout.wide && inside(layout.result) && self.preview != Preview::Base {
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    self.preview = Preview::Result;
                }
                if self.preview == Preview::Result {
                    return EventResult::Ignored(None);
                }
            }
            if let Some(side) = side {
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    self.preview = if side == 1 {
                        Preview::Current
                    } else if side == 2 {
                        Preview::Incoming
                    } else {
                        Preview::Base
                    };
                }
                match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        self.scroll[side] = self.scroll[side].saturating_add(3)
                    }
                    MouseEventKind::ScrollUp => {
                        self.scroll[side] = self.scroll[side].saturating_sub(3)
                    }
                    _ => {}
                }
                return EventResult::Consumed(None);
            }
        }
        if self.preview != Preview::Result {
            if let Event::Key(key) = event {
                self.source_key(key.code);
            }
            return EventResult::Consumed(None);
        }
        EventResult::Ignored(None)
    }
}
#[cfg(test)]
mod test;
