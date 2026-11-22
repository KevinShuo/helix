//! AI actions and staged session settings.
mod actions;
mod local_models;

use crate::compositor::{Component, Context, Event, EventResult};
use crate::{ctrl, key};
use helix_view::{
    ai::{AiFeature, AiProviderConfig, AiProviderType, AiThinking},
    editor::{Config, ConfigEvent},
    graphics::Rect,
    Editor,
};
use tui::{
    buffer::Buffer,
    widgets::{Block, Borders, Widget},
};

pub(crate) struct Panel {
    actions: actions::Actions,
    settings: Settings,
    show_settings: bool,
}

impl Panel {
    pub(crate) fn new(editor: &Editor) -> Self {
        Self {
            actions: actions::Actions::new(editor),
            settings: Settings::new(&editor.config()),
            show_settings: false,
        }
    }
}

impl Component for Panel {
    fn id(&self) -> Option<&'static str> {
        Some("ai-settings")
    }

    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return match event {
                Event::Mouse(_) | Event::Paste(_) => EventResult::Consumed(None),
                _ => EventResult::Ignored(None),
            };
        };
        if self.show_settings {
            if self.settings.local_models.is_none() && matches!(*key, key!(Tab) | key!('a')) {
                self.show_settings = false;
                return EventResult::Consumed(None);
            }
            return self.settings.handle_event(event, cx);
        }
        if matches!(*key, key!(Tab) | key!('s')) {
            self.show_settings = true;
            self.settings.message.clear();
            return EventResult::Consumed(None);
        }
        if *key == key!(Enter) {
            if let Some(row) = self.actions.settings_row(cx.editor) {
                self.show_settings = true;
                self.settings.row = row;
                self.settings.message = "空格切换，Enter 应用".into();
                return EventResult::Consumed(None);
            }
            if self.settings.draft != self.settings.original {
                self.show_settings = true;
                self.settings.message = "设置有修改 · Enter 应用或 Esc 放弃后再执行操作".into();
                return EventResult::Consumed(None);
            }
        }
        self.actions.handle_event(event, cx)
    }

    fn render(&mut self, viewport: Rect, surface: &mut Buffer, cx: &mut Context) {
        if self.show_settings {
            self.settings.render(viewport, surface, cx);
        } else {
            self.actions.render(viewport, surface, cx);
        }
    }
}

struct Settings {
    original: Config,
    draft: Config,
    row: usize,
    message: String,
    local_models: Option<local_models::Choices>,
}

impl Settings {
    fn new(config: &Config) -> Self {
        Self {
            original: config.clone(),
            draft: config.clone(),
            row: 0,
            message: String::new(),
            local_models: None,
        }
    }

    fn generation_row(&self) -> usize {
        self.draft.ai_features().len() + 1
    }

    fn next_model(config: &mut Config, feature: AiFeature) {
        let mut choices = config.ai.providers.clone();
        let cloud = config
            .ai_provider(feature)
            .ok()
            .filter(|p| p.kind == AiProviderType::Deepseek)
            .or_else(|| {
                choices
                    .values()
                    .find(|p| p.kind == AiProviderType::Deepseek)
                    .cloned()
            })
            .unwrap_or_else(AiProviderConfig::default_deepseek);
        for model in ["deepseek-flash", "deepseek-v4-pro"] {
            if choices
                .values()
                .any(|p| p.kind == AiProviderType::Deepseek && p.model == model)
            {
                continue;
            }
            let mut name = model.to_owned();
            while choices.contains_key(&name) {
                name.push('-');
            }
            let mut provider = cloud.clone();
            provider.model = model.into();
            choices.insert(name, provider);
        }
        let local_only = feature == AiFeature::Completion
            && (config.ai_completion.local_only || config.ai.auto_local_provider.is_some());
        choices.retain(|name, p| {
            if local_only {
                p.is_local_completion_model()
                    && (!config.deepseek_only()
                        || config.ai.auto_local_provider.is_none()
                        || p.gpu_layers == 0)
            } else {
                (!config.requires_deepseek(feature) || p.kind == AiProviderType::Deepseek)
                    && (feature == AiFeature::Completion
                        || !p.is_local_completion_model()
                        || config.is_generation_choice(name))
            }
        });
        if local_only && choices.is_empty() {
            return;
        }
        let names: Vec<_> = (!local_only)
            .then_some(None)
            .into_iter()
            .chain(choices.keys().cloned().map(Some))
            .collect();
        let (_, selected) = config.ai_feature_settings(feature);
        let next = names[names
            .iter()
            .position(|name| name == selected)
            .map_or(0, |index| (index + 1) % names.len())]
        .clone();
        *selected = next.clone();
        if let Some(name) = next {
            config
                .ai
                .providers
                .entry(name.clone())
                .or_insert_with(|| choices.remove(&name).unwrap());
        }
    }

    fn edit(&mut self, key: char) -> anyhow::Result<()> {
        let mut config = self.draft.clone();
        if self.row == 0 {
            if key != ' ' {
                return Ok(());
            }
            config.ai.enabled = !config.ai.enabled;
        } else {
            let (feature, enabled) = config.ai_features()[self.row - 1];
            match key {
                ' ' => *config.ai_feature_settings(feature).0 = !enabled,
                _ if !config.ai.enabled || !enabled => anyhow::bail!("请先开启总开关和此功能"),
                'm' => Self::next_model(&mut config, feature),
                't' => {
                    anyhow::ensure!(
                        config.ai_provider(feature)?.kind != AiProviderType::OpenaiCompatible,
                        "此线路未配置思考参数支持"
                    );
                    let level = match config.ai.thinking.get(&feature) {
                        None => Some(AiThinking::Off),
                        Some(AiThinking::Off) => Some(AiThinking::Low),
                        Some(AiThinking::Low) => Some(AiThinking::High),
                        Some(AiThinking::High) => Some(AiThinking::Max),
                        Some(AiThinking::Max) => None,
                    };
                    if let Some(level) = level {
                        config.ai.thinking.insert(feature, level);
                    } else {
                        config.ai.thinking.remove(&feature);
                    }
                }
                _ => return Ok(()),
            }
        }
        crate::ai_hardware::configure(&mut config)?;
        self.draft = config;
        Ok(())
    }
}

impl Component for Settings {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        self.message.clear();
        if let Some(choices) = &mut self.local_models {
            match *key {
                key!(Esc) | ctrl!('c') => self.local_models = None,
                key!(Enter) => {
                    let mut draft = self.draft.clone();
                    draft.ai.local_generation_provider = choices.selected().cloned();
                    match crate::ai_hardware::configure(&mut draft) {
                        Ok(()) => {
                            self.draft = draft;
                            self.local_models = None;
                        }
                        Err(error) => self.message = error.to_string(),
                    }
                }
                key => choices.navigate(key),
            }
            return EventResult::Consumed(None);
        }
        let generation_row = self.generation_row();
        let rows = generation_row + 1;
        match *key {
            key!(Up) | key!('k') => self.row = (self.row + rows - 1) % rows,
            key!(Down) | key!('j') => self.row = (self.row + 1) % rows,
            key!(End) => self.row = generation_row,
            key!(Home) => self.row = 0,
            key!(' ') | key!('m') if self.row == generation_row => {
                self.local_models = Some(local_models::Choices::new(&self.draft));
            }
            key!('t') if self.row == generation_row => {}
            key!(' ') | key!('m') | key!('t') => {
                if let helix_view::keyboard::KeyCode::Char(ch) = key.code {
                    if let Err(error) = self.edit(ch) {
                        self.message = error.to_string();
                    }
                }
            }
            key!(Enter) => {
                if *cx.editor.config() != self.original {
                    self.message = "配置已更新，请按 Esc 关闭并重新打开面板".into();
                    return EventResult::Consumed(None);
                }
                if self.draft != self.original {
                    if let Err(err) = cx
                        .editor
                        .config_events
                        .0
                        .send(ConfigEvent::Update(Box::new(self.draft.clone())))
                    {
                        self.message = err.to_string();
                        return EventResult::Consumed(None);
                    }
                }
                return EventResult::Consumed(Some(Box::new(|c, _| {
                    c.remove("ai-settings");
                })));
            }
            key!(Esc) | ctrl!('c') => {
                return EventResult::Consumed(Some(Box::new(|c, _| {
                    c.remove("ai-settings");
                })))
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, viewport: Rect, surface: &mut Buffer, cx: &mut Context) {
        if let Some(choices) = &mut self.local_models {
            choices.render(viewport, surface, cx.editor, &self.draft, &self.message);
            return;
        }
        let width = viewport.width.min(104);
        let height = viewport.height.min(15);
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
            .title(" AI 工作台 · 会话设置 ")
            .style(style);
        let body = block.inner(area);
        block.render(area, surface);
        if body.width == 0 || body.height == 0 {
            return;
        }
        let mut lines = vec![format!(
            "总开关   {}",
            if self.draft.ai.enabled {
                "开启"
            } else {
                "关闭"
            }
        )];
        for (feature, enabled) in self.draft.ai_features() {
            let label = match feature {
                AiFeature::Completion => "行内补全",
                AiFeature::Doc => "函数文档",
                AiFeature::Commit => "Git 提交",
                AiFeature::Test => "测试生成",
                AiFeature::Merge => "合并辅助",
                AiFeature::Explain => "选区问答",
                AiFeature::Generate => "光标生成",
            };
            let state = if enabled { "开" } else { "关" };
            let mut text = format!("{label}   {state}");
            if feature == AiFeature::Completion && enabled && self.draft.ai.enabled {
                if let Some(reason) = &self.draft.ai_completion.unavailable {
                    text.push_str(&format!("   {reason}"));
                }
            }
            if enabled && self.draft.ai.enabled {
                if let Ok(p) = self.draft.ai_provider(feature) {
                    let thinking = if p.kind == AiProviderType::OpenaiCompatible {
                        "不支持".into()
                    } else if let Some(level) = self.draft.ai.thinking.get(&feature) {
                        level.label().into()
                    } else if p.thinking {
                        format!(
                            "跟随模型 ({})",
                            p.reasoning_effort.as_deref().unwrap_or("开启")
                        )
                    } else {
                        "跟随模型 (关)".into()
                    };
                    text.push_str(&format!(
                        "   {} / {}   思考 {}",
                        p.route_label(),
                        p.model,
                        thinking
                    ));
                }
            }
            lines.push(text);
        }
        let selected = self.draft.ai.local_generation_provider.as_deref();
        let selected_model = selected.map_or_else(
            || local_models::AUTO.to_owned(),
            |name| local_models::model_label(&self.draft, name),
        );
        lines.push(format!("默认本地生成模型   {selected_model}"));
        let capacity = (body.height as usize).saturating_sub(4).max(1);
        let first = (self.row + 1).saturating_sub(capacity);
        for (i, text) in lines.iter().enumerate().skip(first).take(capacity) {
            let selected = i == self.row;
            let line = format!("{} {text}", if selected { "›" } else { " " });
            let row_style = if selected {
                cx.editor.theme.get("ui.menu.selected")
            } else {
                style
            };
            surface.set_stringn(
                body.x,
                body.y + (i - first) as u16,
                &line,
                body.width as usize,
                row_style,
            );
        }
        let mut hint = self.message.clone();
        if hint.is_empty() {
            if self.row == self.generation_row() {
                hint.push_str("选择仅本次会话生效   ");
            } else if let Some((feature, _)) = self
                .row
                .checked_sub(1)
                .map(|index| self.draft.ai_features()[index])
            {
                let inherited = if feature != AiFeature::Completion
                    && self.draft.ai.local_generation_provider.is_some()
                {
                    "跟随默认本地生成模型"
                } else {
                    match feature {
                        AiFeature::Explain | AiFeature::Generate => "跟随生成模型",
                        _ => "跟随默认线路",
                    }
                };
                let name = self
                    .draft
                    .feature_provider_name(feature)
                    .map_or(inherited, String::as_str);
                hint = format!("模型选择　{name}   ");
            }
            if self.draft.laptop() {
                hint.push_str("笔记本 · 除行内补全外均使用 DeepSeek");
            } else if self.draft.deepseek_only() {
                hint.push_str(if self.draft.ai_completion.local_only {
                    "行内补全仅本地 · 其他功能仅 DeepSeek"
                } else {
                    "专用显存不足或未知 · 仅 DeepSeek"
                });
            }
        }
        let edit_hint = if self.row == self.generation_row() {
            "↑↓ 选择   m / 空格 浏览本地生成模型"
        } else {
            "↑↓ 选择   空格 开关   m 模型   t 思考"
        };
        for (i, line) in [hint.as_str(), edit_hint, "Enter 应用   Esc 取消   Tab 操作"]
            .iter()
            .enumerate()
        {
            if body.height > 3 {
                surface.set_stringn(
                    body.x,
                    body.bottom() - 3 + i as u16,
                    *line,
                    body.width as usize,
                    style,
                );
            }
        }
    }
}
