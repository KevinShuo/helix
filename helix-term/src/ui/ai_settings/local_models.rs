use crate::{key, ui::ai_preview_text::display};
use helix_view::{editor::Config, graphics::Rect, input::KeyEvent, Editor};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

pub(super) const AUTO: &str = "自动选择";

pub(super) fn model_label(config: &Config, name: &str) -> String {
    match config.ai.providers.get(name) {
        Some(provider) if provider.model != name => {
            format!("{}   ({})", display(&provider.model), display(name))
        }
        Some(_) => display(name),
        None => format!("{} · 本机未发现", display(name)),
    }
}

pub(super) struct Choices {
    names: Vec<String>,
    row: usize,
    page_size: usize,
}

impl Choices {
    pub(super) fn new(config: &Config) -> Self {
        let names: Vec<_> = config
            .ai
            .providers
            .keys()
            .filter(|name| config.is_generation_choice(name))
            .cloned()
            .collect();
        let row = config
            .ai
            .local_generation_provider
            .as_ref()
            .and_then(|selected| names.iter().position(|name| name == selected))
            .map_or(0, |index| index + 1);
        Self {
            names,
            row,
            page_size: 1,
        }
    }

    pub(super) fn selected(&self) -> Option<&String> {
        self.row.checked_sub(1).map(|i| &self.names[i])
    }

    pub(super) fn navigate(&mut self, key: KeyEvent) {
        let last = self.names.len();
        self.row = match key {
            key!(Up) | key!('k') => {
                if self.row == 0 {
                    last
                } else {
                    self.row - 1
                }
            }
            key!(Down) | key!('j') => (self.row + 1) % (last + 1),
            key!(PageUp) => self.row.saturating_sub(self.page_size),
            key!(PageDown) => (self.row + self.page_size).min(last),
            key!(Home) => 0,
            key!(End) => last,
            _ => self.row,
        };
    }

    pub(super) fn render(
        &mut self,
        viewport: Rect,
        surface: &mut Buffer,
        editor: &Editor,
        config: &Config,
        error: &str,
    ) {
        let count = self.names.len() + 1;
        let width = viewport.width.min(104);
        let height = viewport
            .height
            .min((count.saturating_add(5).min(20)) as u16);
        let area = Rect::new(
            viewport.x + (viewport.width - width) / 2,
            viewport.y + (viewport.height - height) / 2,
            width,
            height,
        );
        let style = editor.theme.get("ui.popup");
        surface.clear_with(area, style);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 默认本地生成模型 · 当前会话 ")
            .style(style);
        let body = block.inner(area);
        block.render(area, surface);
        if body.width == 0 || body.height == 0 {
            return;
        }
        let footer = body.height.saturating_sub(1).min(3);
        self.page_size = usize::from(body.height - footer);
        let first = (self.row + 1).saturating_sub(self.page_size);
        for (y, index) in (first..count).take(self.page_size).enumerate() {
            let name = index.checked_sub(1).map(|i| &self.names[i]);
            let title = name.map_or_else(|| AUTO.to_owned(), |name| model_label(config, name));
            let active = name == config.ai.local_generation_provider.as_ref();
            let focused = index == self.row;
            let marker = if focused { "›" } else { " " };
            let saved = if active { "  ✓" } else { "" };
            let row_style = if focused {
                editor.theme.get("ui.menu.selected")
            } else {
                style
            };
            surface.set_stringn(
                body.x,
                body.y + y as u16,
                &format!("{marker} {title}{saved}"),
                body.width as usize,
                row_style,
            );
        }
        let detail = if !error.is_empty() {
            error.to_owned()
        } else if let Some(name) = self.selected() {
            let mut detail = format!("Provider ID   {}", display(name));
            if let Some(path) = &config.ai.providers[name].model_path {
                detail.push_str("   ");
                detail.push_str(&display(&path.to_string_lossy()));
            }
            detail
        } else if self.names.is_empty() {
            "未找到本地生成模型 · 可在 editor.ai.local-model-directories 中指定 GGUF 目录".into()
        } else {
            "跟随功能配置和本机显存".into()
        };
        let policy = if config.laptop() {
            "笔记本 · 生成任务使用 DeepSeek"
        } else if config.deepseek_only() {
            "专用显存不足或未知 · 生成任务使用 DeepSeek"
        } else {
            "Enter 暂存选择，返回设置后按 Enter 应用"
        };
        let help = format!(
            "↑↓ 选择   Enter 暂存   Esc 返回   {}/{}",
            self.row + 1,
            count
        );
        let lines = [detail.as_str(), policy, help.as_str()];
        for (y, line) in lines[3 - usize::from(footer)..].iter().enumerate() {
            surface.set_stringn(
                body.x,
                body.bottom() - footer + y as u16,
                line,
                body.width as usize,
                style,
            );
        }
    }
}
