use crate::{
    commands::typed::{execute_command, TYPABLE_COMMAND_MAP},
    compositor::{Component, Context, Event, EventResult},
    ctrl, handlers, key, shift,
    ui::{ai_preview_text::display, PromptEvent},
};
use helix_view::{
    ai::AiFeature, ai_chat::Scope, document::Mode, editor::Config, graphics::Rect, Editor,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Action {
    label: &'static str,
    command: Option<&'static str>,
    feature: Option<AiFeature>,
}

impl Action {
    const fn new(label: &'static str, command: &'static str, feature: Option<AiFeature>) -> Self {
        Self {
            label,
            command: Some(command),
            feature,
        }
    }

    fn settings_row(self, config: &Config) -> Option<usize> {
        let feature = self.feature?;
        if !config.ai.enabled && self != INLINE {
            return Some(0);
        }
        config
            .ai_features()
            .iter()
            .position(|&(f, enabled)| f == feature && (!enabled || self == INLINE))
            .map(|i| i + 1)
    }

    fn unavailable(self, editor: &Editor) -> Option<&'static str> {
        (self == MERGE && doc!(editor).merge().is_none())
            .then_some("需要冲突文件 · Space v r 打开合并视图")
    }

    fn activity(self, editor: &Editor) -> Option<&'static str> {
        match self {
            EXPLAIN | OPTIMIZE => editor.ai_chat.as_ref().and_then(|chat| {
                (chat.pending.is_some() && chat.optimize == (self == OPTIMIZE))
                    .then_some("正在回答")
            }),
            GENERATE if handlers::ai_generate::active(editor) => {
                let op = doc!(editor).ai_generate.as_ref()?;
                Some(if op.failed {
                    "生成失败 · 可重试"
                } else if op.annotation.is_some() {
                    "等待接受"
                } else {
                    "生成中"
                })
            }
            DOC => Some(if editor.ai_doc.as_ref()?.text.is_some() {
                "等待审阅"
            } else {
                "生成中"
            }),
            COMMIT if editor.ai_commit_running => Some("分析中"),
            MERGE => Some(if editor.ai_merge.as_ref()?.suggestion.is_some() {
                "等待审阅"
            } else {
                "生成中"
            }),
            TEST => {
                let op = editor.ai_test.as_ref()?;
                if op.running {
                    Some("测试运行中")
                } else if op.applied {
                    Some("测试已生成")
                } else if op.changes.is_some() {
                    Some("等待审阅")
                } else {
                    Some("生成中")
                }
            }
            _ => None,
        }
    }
}

const EXPLAIN: Action = Action::new("解释代码", "ai-explain", Some(AiFeature::Explain));
const OPTIMIZE: Action = Action::new("优化代码", "ai-optimize", Some(AiFeature::Explain));
const ASK: Action = Action::new("对代码提问", "ai-ask", Some(AiFeature::Explain));
const GENERATE: Action = Action::new(
    "在光标处生成  Ctrl+\\",
    "ai-generate",
    Some(AiFeature::Generate),
);
const DOC: Action = Action::new("生成函数文档", "ai-doc", Some(AiFeature::Doc));
const TEST: Action = Action::new("生成函数测试", "ai-test", Some(AiFeature::Test));
const COMMIT: Action = Action::new("准备 Git 提交", "ai-commit", Some(AiFeature::Commit));
const MERGE: Action = Action::new("处理选中冲突", "ai-merge", Some(AiFeature::Merge));
const SHOW_CHAT: Action = Action::new("返回最近问答", "ai-explain-show", None);
const SHOW_OPTIMIZE: Action = Action::new("返回最近优化建议", "ai-explain-show", None);
const SHOW_TEST: Action = Action::new("查看测试结果", "ai-test-show", None);
const RUN_TEST: Action = Action::new("运行已生成测试", "ai-test-run", None);
const INLINE: Action = Action {
    label: "行内补全",
    command: None,
    feature: Some(AiFeature::Completion),
};

fn available(editor: &Editor) -> Vec<Action> {
    let mut actions = vec![
        EXPLAIN, OPTIMIZE, ASK, GENERATE, DOC, TEST, COMMIT, MERGE, INLINE,
    ];
    if let Some(chat) = &editor.ai_chat {
        actions.push(if chat.optimize {
            SHOW_OPTIMIZE
        } else {
            SHOW_CHAT
        });
        if chat.pending.is_some() {
            actions.push(Action::new("取消当前问答", "ai-explain-cancel", None));
        }
    }
    if handlers::ai_generate::active(editor) {
        actions.push(Action::new("取消光标生成", "ai-generate-cancel", None));
    }
    if let Some(test) = &editor.ai_test {
        if test.changes.is_some() || test.report.is_some() {
            actions.push(SHOW_TEST);
        }
        if test.applied && !test.running {
            actions.push(RUN_TEST);
        }
        if handlers::ai_test::busy(editor) {
            actions.push(Action::new("取消测试任务", "ai-test-cancel", None));
        }
    }
    if editor.ai_doc.is_some() {
        actions.push(Action::new("取消文档生成", "ai-doc-cancel", None));
    }
    if editor.ai_commit_running {
        actions.push(Action::new("取消提交分析", "ai-commit-cancel", None));
    }
    if editor.ai_merge.is_some() {
        actions.push(Action::new("取消合并辅助", "ai-merge-cancel", None));
    }
    actions
}

pub(super) struct Actions {
    selected: Action,
    context: String,
    message: String,
    explain_scope: usize,
    optimize_scope: usize,
    symbol_name: &'static str,
    project_root: String,
}

const SCOPES: [(Scope, &str, &str); 4] = [
    (Scope::Selection, "selection", "选区"),
    (Scope::Function, "function", "函数 / 声明"),
    (Scope::File, "file", "当前文件"),
    (Scope::Project, "project", "工程"),
];

impl Actions {
    pub(super) fn new(editor: &Editor) -> Self {
        let (view, doc) = current_ref!(editor);
        let selection = doc.selection(view.id);
        let primary = selection.primary();
        let text = doc.text().slice(..);
        let has_selection = editor.mode() == Mode::Select
            || (!primary.is_empty() && !primary.is_single_grapheme(text));
        let selected = if doc.merge().is_some() {
            MERGE
        } else if has_selection {
            EXPLAIN
        } else {
            GENERATE
        };
        let (first, last) = primary.line_range(text);
        let location = if selection.len() > 1 {
            format!(
                "{} 处选区 · 主选区 {} 行",
                selection.len(),
                last - first + 1
            )
        } else if has_selection {
            format!(
                "选中 {} 行 · 第 {}–{} 行",
                last - first + 1,
                first + 1,
                last + 1
            )
        } else {
            format!("光标位于第 {} 行", first + 1)
        };
        let context = format!("{location}   {}", display(&doc.display_name()));
        let target = handlers::ai_scope::auto_target(editor);
        let initial = usize::from(matches!(target, Some((Scope::Function, _))));
        let project_root = match handlers::ai_scope::project_root(editor) {
            Ok(path) => display(&helix_stdx::path::normalize(path).to_string_lossy()),
            Err(error) => display(&error.to_string()),
        };
        Self {
            selected,
            context,
            message: String::new(),
            explain_scope: initial,
            optimize_scope: initial,
            symbol_name: target
                .filter(|(scope, _)| *scope == Scope::Function)
                .map_or("函数 / 声明", |(_, label)| label),
            project_root,
        }
    }

    pub(super) fn has_scopes(&self) -> bool {
        matches!(self.selected, EXPLAIN | OPTIMIZE)
    }

    fn scope(&self) -> (Scope, &'static str, &'static str) {
        SCOPES[if self.selected == OPTIMIZE {
            self.optimize_scope
        } else {
            self.explain_scope
        }]
    }

    fn cycle_scope(&mut self, forward: bool) {
        let (index, count) = if self.selected == OPTIMIZE {
            (&mut self.optimize_scope, SCOPES.len() - 1)
        } else {
            (&mut self.explain_scope, SCOPES.len())
        };
        *index = (*index + if forward { 1 } else { count - 1 }) % count;
    }

    pub(super) fn settings_row(&self, editor: &Editor) -> Option<usize> {
        if self.selected.unavailable(editor).is_some() {
            return None;
        }
        self.selected.settings_row(&editor.config())
    }
}

impl Component for Actions {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let rows = available(cx.editor);
        let index = rows.iter().position(|&action| action == self.selected);
        self.message.clear();
        let next = match *key {
            key!(Tab) | shift!(Tab) | key!(Right) | key!(Left) if self.has_scopes() => {
                self.cycle_scope(matches!(*key, key!(Tab) | key!(Right)));
                return EventResult::Consumed(None);
            }
            key!(Up) | key!('k') => (index.unwrap_or(0) + rows.len() - 1) % rows.len(),
            key!(Down) | key!('j') => (index.unwrap_or(0) + 1) % rows.len(),
            key!(Enter) if index.is_some() => {
                if self.selected.unavailable(cx.editor).is_some() {
                    return EventResult::Consumed(None);
                }
                let Some(command) = self.selected.command else {
                    return EventResult::Consumed(None);
                };
                let args = if self.has_scopes() {
                    self.scope().1
                } else {
                    ""
                };
                if let Err(error) = execute_command(
                    cx,
                    TYPABLE_COMMAND_MAP[command],
                    args,
                    PromptEvent::Validate,
                ) {
                    self.message = display(&error.to_string());
                    return EventResult::Consumed(None);
                }
                return EventResult::Consumed(Some(Box::new(|compositor, _| {
                    compositor.remove("ai-settings");
                })));
            }
            key!(Esc) | ctrl!('c') => {
                return EventResult::Consumed(Some(Box::new(|compositor, _| {
                    compositor.remove("ai-settings");
                })));
            }
            _ => return EventResult::Consumed(None),
        };
        self.selected = rows[next];
        EventResult::Consumed(None)
    }

    fn render(&mut self, viewport: Rect, surface: &mut Buffer, cx: &mut Context) {
        let rows = available(cx.editor);
        let index = rows.iter().position(|&a| a == self.selected).unwrap_or(0);
        self.selected = rows[index];
        let width = viewport.width.min(80);
        let scoped = self.has_scopes();
        let height = viewport
            .height
            .min(rows.len() as u16 + 5 + if scoped { 3 } else { 0 });
        let area = Rect::new(
            viewport.x + (viewport.width - width) / 2,
            viewport.y + (viewport.height - height) / 2,
            width,
            height,
        );
        let style = cx.editor.theme.get("ui.popup");
        surface.clear_with(area, style);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" AI 工作台 · 操作 ")
            .style(style);
        let mut body = block.inner(area);
        block.render(area, surface);
        if body.width == 0 || body.height == 0 {
            return;
        }
        if body.height > 3 {
            surface.set_stringn(body.x, body.y, &self.context, body.width as usize, style);
            body = body.clip_top(1);
        }
        let config = cx.editor.config();
        if scoped && body.height >= 5 {
            let (scope, _, current) = self.scope();
            let scope_line = if body.width >= 48 {
                let mut line = String::from("范围 ");
                for &(value, _, label) in &SCOPES {
                    if self.selected == OPTIMIZE && value == Scope::Project {
                        continue;
                    }
                    let label = if value == Scope::Function {
                        self.symbol_name
                    } else {
                        label
                    };
                    if scope == value {
                        line.push_str(&format!(" [{label}]"));
                    } else {
                        line.push_str(&format!("  {label}"));
                    }
                }
                line
            } else {
                format!("范围  {current} · Tab 切换")
            };
            let model = config
                .ai_provider(AiFeature::Explain)
                .map(|p| format!("{} · {}", display(&p.model), p.route_label()))
                .unwrap_or_else(|e| display(&e.to_string()));
            surface.set_stringn(body.x, body.y, &scope_line, body.width as usize, style);
            body = body.clip_top(1);
            let (x, _) =
                surface.set_stringn(body.x, body.y, "工程根  ", body.width as usize, style);
            surface.set_string_truncated(
                x,
                body.y,
                &self.project_root,
                body.right().saturating_sub(x) as usize,
                |_| style,
                true,
                true,
            );
            body = body.clip_top(1);
            surface.set_stringn(
                body.x,
                body.y,
                &format!("模型  {model}"),
                body.width as usize,
                style,
            );
            body = body.clip_top(1);
        }
        let visible = body.height.saturating_sub(2).max(1) as usize;
        let first = (index + 1).saturating_sub(visible);
        for (offset, action) in rows.iter().skip(first).take(visible).enumerate() {
            let selected = offset + first == index;
            let marker = if selected { "›" } else { " " };
            let state = if *action == INLINE {
                Some(if config.ai.enabled && config.ai_completion.enabled {
                    "开"
                } else {
                    "关"
                })
            } else if action.unavailable(cx.editor).is_some() {
                Some("需要冲突文件")
            } else {
                action
                    .activity(cx.editor)
                    .or_else(|| action.settings_row(&config).is_some().then_some("已关闭"))
            };
            let mut line = format!("{marker} {}", action.label);
            if selected && scoped {
                line.push_str(" · ");
                line.push_str(self.scope().2);
            }
            if let Some(command) = action.command {
                line.push_str("   :");
                line.push_str(command);
            }
            if let Some(state) = state {
                line.push_str(" · ");
                line.push_str(state);
            }
            let row_style = if selected {
                cx.editor.theme.get("ui.menu.selected")
            } else {
                style
            };
            surface.set_stringn(
                body.x,
                body.y + offset as u16,
                &line,
                body.width as usize,
                row_style,
            );
        }
        if body.height > 2 {
            let hint = if !self.message.is_empty() {
                &self.message
            } else if let Some(reason) = self.selected.unavailable(cx.editor) {
                reason
            } else if self.selected == INLINE {
                "Enter 前往行内补全设置"
            } else {
                match self.selected.settings_row(&config) {
                    Some(0) => "AI 总开关已关闭 · Enter 前往设置",
                    Some(_) => "此功能已关闭 · Enter 前往设置",
                    None => "Enter 执行所选操作",
                }
            };
            for (y, text) in [
                (body.bottom() - 2, hint),
                (
                    body.bottom() - 1,
                    if scoped && body.width < 40 {
                        "Tab 范围 · s 设置 · Esc"
                    } else if scoped {
                        "Tab/←→ 范围   ↑↓ 选择   s 设置   Esc 关闭"
                    } else {
                        "↑↓ 选择   Tab/s 设置   Esc 关闭"
                    },
                ),
            ] {
                surface.set_stringn(body.x, y, text, body.width as usize, style);
            }
        }
    }
}
