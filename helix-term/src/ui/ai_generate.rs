use super::{Prompt, PromptEvent};
use crate::{
    compositor::{Component, Context, Event, EventResult},
    handlers::ai_generate,
    key,
};
use helix_core::Position;
use helix_event::TaskHandle;
use helix_view::{
    ai::AiProviderConfig,
    graphics::{CursorKind, Rect},
    Editor,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

pub(crate) const ID: &str = "ai-generate-prompt";

pub(crate) struct Dialog {
    prompt: Prompt,
    refining: bool,
}

impl Dialog {
    pub(crate) fn new(
        model: AiProviderConfig,
        handle: TaskHandle,
        refining: bool,
        initial_input: String,
        editor: &Editor,
    ) -> Self {
        let prompt = Prompt::new(
            "".into(),
            None,
            |_, _| vec![],
            move |cx, line, event| {
                if handle.is_canceled() {
                    return;
                }
                match event {
                    PromptEvent::Abort => ai_generate::cancel(cx.editor),
                    PromptEvent::Validate => {
                        if let Err(error) =
                            ai_generate::submit(cx.editor, line, model.clone(), handle.clone())
                        {
                            cx.editor.set_error(error.to_string());
                        }
                    }
                    PromptEvent::Update => {}
                }
            },
        )
        .with_line(initial_input, editor);
        Self { prompt, refining }
    }

    fn area(screen: Rect, editor: &Editor) -> Rect {
        let (view, doc) = current_ref!(editor);
        let area = screen.intersection(view.inner_area(doc));
        let width = area.width.min(66);
        let height = area.height.min(3);
        let pos = editor
            .cursor()
            .0
            .unwrap_or(Position::new(area.y as usize, area.x as usize));
        let x = (pos.col as u16).clamp(area.left(), area.right() - width);
        let below = (pos.row as u16).saturating_add(1);
        let y = if below.saturating_add(height) <= area.bottom() {
            below
        } else {
            (pos.row as u16).saturating_sub(height)
        }
        .clamp(area.top(), area.bottom() - height);
        Rect::new(x, y, width, height)
    }

    fn input_area(area: Rect) -> Rect {
        if area.height >= 3 && area.width >= 8 {
            area.clip_left(1).clip_right(1).clip_top(1).clip_bottom(1)
        } else {
            area.with_height(area.height.min(1))
        }
    }
}

impl Component for Dialog {
    fn id(&self) -> Option<&'static str> {
        Some(ID)
    }

    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if matches!(event, Event::Key(key!(Enter))) && self.prompt.line().trim().is_empty() {
            cx.editor.set_status("请输入生成需求");
            return EventResult::Consumed(None);
        }
        self.prompt.handle_event(event, cx)
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        let area = Self::area(area, cx.editor);
        if area.height == 0 || area.width < 4 {
            return;
        }
        let style = cx.editor.theme.get("ui.popup");
        surface.clear_with(area, style);
        if area.height >= 3 && area.width >= 8 {
            let title = match (self.refining, area.width >= 44) {
                (true, true) => " 补充需求 · Enter 重新生成 · Esc 放弃 ",
                (false, true) => " 描述要生成的代码 · Enter 生成 · Esc 取消 ",
                (true, false) => " 补充需求 ",
                (false, false) => " 生成代码 ",
            };
            Block::default()
                .borders(Borders::ALL)
                .border_style(style)
                .title(title)
                .render(area, surface);
        }
        self.prompt.render(Self::input_area(area), surface, cx);
    }

    fn cursor(&self, area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        let area = Self::input_area(Self::area(area, editor));
        if area.height == 0 || area.width < 4 {
            return (None, CursorKind::Hidden);
        }
        self.prompt.cursor(area, editor)
    }
}
