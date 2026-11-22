use crate::ui::{ai_preview_text::display, markdown::Markdown};
use arc_swap::ArcSwap;
use helix_core::syntax::Loader;
use helix_core::unicode::{segmentation::UnicodeSegmentation, width::UnicodeWidthStr};
use helix_view::{
    ai_chat::Session,
    graphics::{Rect, Style},
    Editor, Theme,
};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
use std::{
    collections::VecDeque,
    ops::{Range, RangeInclusive},
    sync::{Arc, LazyLock},
};
use tui::{
    buffer::{Buffer, SYMBOL_CAPACITY},
    text::{Span, Spans},
};

type Line = Spans<'static>;

fn source_range(label: &str) -> Option<RangeInclusive<usize>> {
    let (_, numbers) = label.rsplit_once(" · 第 ")?;
    let (start, end) = numbers.strip_suffix(" 行")?.split_once('–')?;
    Some(start.parse().ok()?..=end.parse().ok()?)
}

fn line_references(
    text: String,
    style: Style,
    range: Option<&RangeInclusive<usize>>,
    reference: Style,
) -> Vec<Span<'static>> {
    static REFERENCE: LazyLock<helix_core::regex::Regex> = LazyLock::new(|| {
        helix_core::regex::Regex::new(r"第 *([0-9]+)(?: *[-–—至] *([0-9]+))? *行").unwrap()
    });
    let Some(range) = range else {
        return vec![Span::styled(text, style)];
    };
    let mut spans = Vec::new();
    let mut prev = 0;
    for cap in REFERENCE.captures_iter(&text) {
        let Some(start) = cap[1].parse::<usize>().ok() else {
            continue;
        };
        let end = cap.get(2).map_or(Some(start), |n| n.as_str().parse().ok());
        if !range.contains(&start) || !end.is_some_and(|end| end >= start && range.contains(&end)) {
            continue;
        }
        let matched = cap.get(0).unwrap();
        if prev < matched.start() {
            spans.push(Span::styled(text[prev..matched.start()].to_owned(), style));
        }
        spans.push(Span::styled(
            matched.as_str().to_owned(),
            style.patch(reference),
        ));
        prev = matched.end();
    }
    if prev == 0 {
        return vec![Span::styled(text, style)];
    }
    if prev < text.len() {
        spans.push(Span::styled(text[prev..].to_owned(), style));
    }
    spans
}

// Optional sections end at the next top-level H1/H2. Headings in a code block,
// block quote, or list remain part of the answer verbatim.
fn optional_sections(text: &str) -> Vec<(&str, Range<usize>)> {
    if !text.contains("## 更多细节") && !text.contains("## 补充示例") {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut open = None;
    let mut depth = 0;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0
                    && matches!(
                        tag,
                        Tag::Heading {
                            level: HeadingLevel::H1 | HeadingLevel::H2,
                            ..
                        }
                    )
                {
                    if let Some((label, start)) = open.take() {
                        result.push((label, start..range.start));
                    }
                    if let Some(label @ ("更多细节" | "补充示例")) =
                        text[range.clone()].trim().strip_prefix("## ")
                    {
                        open = Some((label, range.start));
                    }
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            _ => {}
        }
    }
    if let Some((label, start)) = open {
        result.push((label, start..text.len()));
    }
    result
}

fn question_lines(question: &str, style: Style) -> Vec<Line> {
    let mut lines: Vec<_> = question
        .lines()
        .map(|line| Spans::from(Span::styled(display(line), style)))
        .collect();
    if let Some(first) = lines.first_mut() {
        first.0.insert(0, Span::styled("你　", style));
    }
    lines.push(Spans::default());
    lines
}

// Wrap by display columns. Keep short words together, preserve indentation and
// fall back to whole graphemes for long identifiers or unspaced Chinese prose.
fn wrap(lines: &[Line], width: u16, rows: &mut Vec<Line>) {
    let width = usize::from(width.max(1));
    for line in lines {
        let mut row = Line::default();
        let mut col = 0;
        for span in &line.0 {
            for word in span.content.split_word_bounds() {
                let w = word.width();
                if col > 0 && w <= width && col + w > width && !word.trim().is_empty() {
                    rows.push(std::mem::take(&mut row));
                    col = 0;
                }
                for g in word.graphemes(true) {
                    let symbol = if g.len() > SYMBOL_CAPACITY || g.width() > width {
                        "�"
                    } else {
                        g
                    };
                    let cells = symbol.width();
                    if col + cells > width {
                        rows.push(std::mem::take(&mut row));
                        col = 0;
                    }
                    if cells != 0 {
                        if row.0.last().is_none_or(|s| s.style != span.style) {
                            row.0.push(Span::styled(String::new(), span.style));
                        }
                        row.0.last_mut().unwrap().content.to_mut().push_str(symbol);
                        col += cells;
                    }
                }
            }
        }
        rows.push(row);
    }
}

struct Section {
    label: Option<String>,
    lines: Vec<Line>,
}

struct Turn {
    original: (String, String),
    question: Vec<Line>,
    sections: Vec<Section>,
    expanded: bool,
    rows: Vec<Line>,
}

impl Turn {
    fn new(
        pair: &(String, String),
        theme: &Theme,
        loader: &Arc<ArcSwap<Loader>>,
        range: Option<&RangeInclusive<usize>>,
    ) -> Self {
        let source = pair.1.replace("\r\n", "\n");
        let safe = source
            .split('\n')
            .map(|line| display(&line.replace('\t', "    ")))
            .collect::<Vec<_>>()
            .join("\n");
        let normal = theme.get("ui.text");
        let reference = theme.get("ui.text.info");
        let parse = |source: &str| {
            let md = Markdown::new(source.to_owned(), loader.clone());
            md.parse(Some(theme))
                .lines
                .into_iter()
                .map(|line| {
                    // Character entities can introduce controls after Markdown parsing.
                    Spans(
                        line.0
                            .into_iter()
                            .flat_map(|s| {
                                line_references(
                                    display(&s.content),
                                    s.style,
                                    range.filter(|_| s.style == normal && s.content.contains('第')),
                                    reference,
                                )
                            })
                            .collect(),
                    )
                })
                .collect()
        };
        let mut sections = Vec::new();
        let mut at = 0;
        for (label, range) in optional_sections(&safe) {
            if at < range.start {
                sections.push(Section {
                    label: None,
                    lines: parse(&safe[at..range.start]),
                });
            }
            let body = safe[range.clone()]
                .split_once('\n')
                .map_or("", |(_, body)| body);
            sections.push(Section {
                label: Some(label.to_owned()),
                lines: parse(body),
            });
            at = range.end;
        }
        if at < safe.len() {
            sections.push(Section {
                label: None,
                lines: parse(&safe[at..]),
            });
        }
        Self {
            original: pair.clone(),
            question: question_lines(&pair.0, reference),
            sections,
            expanded: false,
            rows: Vec::new(),
        }
    }

    fn layout(&mut self, width: u16, marker_style: Style) {
        self.rows.clear();
        wrap(&self.question, width, &mut self.rows);
        for section in &self.sections {
            if let Some(label) = &section.label {
                let marker = if self.expanded { "▾" } else { "▸" };
                wrap(
                    &[Spans::from(Span::styled(
                        format!("{marker} {label}"),
                        marker_style,
                    ))],
                    width,
                    &mut self.rows,
                );
                if !self.expanded {
                    continue;
                }
            }
            wrap(&section.lines, width, &mut self.rows);
        }
        self.rows.push(Spans::default());
    }
}

#[derive(Default)]
pub(super) struct Transcript {
    turns: VecDeque<Turn>,
    tail: Vec<Line>,
    width: u16,
    theme: String,
}

impl Transcript {
    pub(super) fn width(&self) -> u16 {
        self.width
    }

    pub(super) fn theme(&self) -> &str {
        &self.theme
    }

    // Return the number of rows evicted by the shared conversation history limit.
    pub(super) fn update(&mut self, session: &Session, editor: &Editor, width: u16) -> usize {
        if self.theme != editor.theme.name() {
            self.theme = editor.theme.name().to_owned();
            self.turns.clear();
        }
        let mut removed = 0;
        while self
            .turns
            .front()
            .is_some_and(|t| Some(&t.original) != session.turns.first())
        {
            removed += self.turns.pop_front().unwrap().rows.len();
        }
        for (idx, pair) in session.turns.iter().enumerate() {
            if self
                .turns
                .get(idx)
                .is_none_or(|turn| turn.original != *pair)
            {
                self.turns.truncate(idx);
                self.turns.push_back(Turn::new(
                    pair,
                    &editor.theme,
                    &editor.syn_loader,
                    source_range(&session.label).as_ref(),
                ));
            }
            let turn = &mut self.turns[idx];
            if turn.rows.is_empty() || self.width != width {
                turn.layout(width, editor.theme.get("ui.text.info"));
            }
        }
        self.width = width;
        self.tail.clear();
        let style = editor.theme.get("ui.text.info");
        let mut tail = Vec::new();
        if let Some(q) = &session.pending {
            tail = question_lines(q, style);
            if session.agent_progress.is_empty() {
                tail.push(Spans::from(Span::styled("正在回答…", style)));
            }
        } else if !session.notice.is_empty() {
            tail.push(Spans::from(Span::styled(
                display(&session.notice),
                editor.theme.get("warning"),
            )));
        } else if self.turns.is_empty() {
            tail.push(Spans::from(Span::styled(
                "想了解这段代码的哪一部分？",
                style,
            )));
        }
        wrap(&tail, width, &mut self.tail);
        removed
    }

    pub(super) fn len(&self) -> usize {
        self.turns.iter().map(|t| t.rows.len()).sum::<usize>() + self.tail.len()
    }

    pub(super) fn latest(&self) -> usize {
        self.len()
            - if self.tail.is_empty() {
                self.turns.back().map_or(0, |t| t.rows.len())
            } else {
                self.tail.len()
            }
    }

    // If the viewport contains parts of two turns, prefer the newest visible
    // turn with a detail section. At the end of a short conversation the
    // beginning of the new turn may sit below the first row of the viewport.
    pub(super) fn detail_turn(&self, top: usize, height: usize) -> Option<usize> {
        let mut start = 0;
        let mut found = None;
        for (index, turn) in self.turns.iter().enumerate() {
            if start >= top.saturating_add(height) {
                break;
            }
            let end = start + turn.rows.len();
            if end > top && turn.sections.iter().any(|s| s.label.is_some()) {
                found = Some(index)
            }
            start = end;
        }
        found
    }

    pub(super) fn toggle(&mut self, idx: usize, style: Style) {
        let turn = &mut self.turns[idx];
        turn.expanded = !turn.expanded;
        turn.layout(self.width, style);
    }

    pub(super) fn render(&self, area: Rect, surface: &mut Buffer, mut top: usize) {
        let mut y = area.y;
        for block in self
            .turns
            .iter()
            .map(|t| &t.rows)
            .chain(std::iter::once(&self.tail))
        {
            if top >= block.len() {
                top -= block.len();
                continue;
            }
            for row in &block[top..] {
                if y >= area.bottom() {
                    return;
                }
                surface.set_spans(area.x, y, row, area.width);
                y += 1;
            }
            top = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::graphics::{Color, Modifier};

    fn row_text(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|line| line.0.iter().map(|span| &*span.content))
            .collect()
    }

    #[test]
    fn optional_sections_exclude_code_and_end_at_peer_heading() {
        let source = "```md\n## 更多细节\n```\n\n> ## 补充示例\n\n## 更多细节\n解释\n### 内部\n仍在细节中\n## 结论\n始终可见";
        let sections = optional_sections(source);
        assert_eq!(sections.len(), 1);
        let (label, range) = &sections[0];
        assert_eq!(*label, "更多细节");
        assert_eq!(
            &source[range.clone()],
            "## 更多细节\n解释\n### 内部\n仍在细节中\n"
        );
    }

    #[test]
    fn code_and_chinese_wrap_without_losing_spaces_or_splitting_graphemes() {
        let style = Style::default().fg(Color::Blue);
        let source = "    中文 e\u{301}  a_long_identifier";
        let mut rows = Vec::new();
        wrap(&[Line::from(Span::styled(source, style))], 7, &mut rows);
        assert!(rows.iter().all(|line| line.width() <= 7));
        assert_eq!(row_text(&rows), source);
        assert!(rows.iter().flat_map(|r| &r.0).all(|s| s.style == style));
    }

    #[test]
    fn only_references_within_the_selection_get_the_accent() {
        let accent = Style::default().fg(Color::Cyan);
        let range = source_range("example.hpp · 第 67–75 行").unwrap();
        let text = "第 70 行、第71–75行、第 71–80 行、第 5 行";
        let parts = line_references(text.into(), Style::default(), Some(&range), accent);
        let links: Vec<_> = parts
            .iter()
            .filter(|s| s.style == accent)
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(links, ["第 70 行", "第71–75行"]);
    }

    #[test]
    fn parsed_entities_and_raw_controls_stay_inert() {
        let loader = Loader::new(helix_core::syntax::config::Configuration {
            language: vec![],
            language_server: Default::default(),
        })
        .unwrap();
        let pair = (
            "问题".into(),
            "**`value`**\n\n&#x202e;方向\u{1b}[31m\n\n```text\n   **原样**\n```".into(),
        );
        let mut turn = Turn::new(
            &pair,
            &Theme::default(),
            &Arc::new(ArcSwap::from_pointee(loader)),
            None,
        );
        turn.layout(80, Style::default());
        let text = row_text(&turn.rows);
        assert!(!text.contains(['\u{202e}', '\u{1b}']));
        assert!(text.contains("�方向�[31m"), "{text}");
        assert!(text.contains("   **原样**"), "{text}");
        assert!(turn
            .rows
            .iter()
            .flat_map(|line| &line.0)
            .any(|s| s.content == "value" && s.style.add_modifier.contains(Modifier::BOLD)));
    }
}
