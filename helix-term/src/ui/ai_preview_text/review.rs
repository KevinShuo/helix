use super::{display, PreviewText};
use helix_view::{graphics::Rect, theme::Theme};
use imara_diff::{Algorithm, BasicLineDiffPrinter, Diff, InternedInput, UnifiedDiffConfig};
use tui::buffer::Buffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Role {
    Text,
    Heading,
    Note,
    Added,
    Removed,
    Success,
    Failure,
}

#[derive(Default)]
pub(in crate::ui) struct ReviewLines {
    pub(in crate::ui) source: String,
    roles: Vec<Role>,
}

impl ReviewLines {
    pub(in crate::ui) fn push(&mut self, role: Role, text: &str) {
        if !self.roles.is_empty() {
            self.source.push('\n');
        }
        self.source.push_str(text);
        self.roles.extend(text.split('\n').map(|_| role));
    }

    pub(in crate::ui) fn finish(self) -> ReviewText {
        ReviewText {
            text: PreviewText::new(self.source),
            roles: self.roles,
        }
    }

    /// Preserve complete context for a merge candidate. Multi-file test reviews
    /// use three context lines on either side of each modified region.
    pub(in crate::ui) fn diff(&mut self, before: &str, after: &str, complete: bool) {
        let input = InternedInput::new(before, after);
        let diff = Diff::compute(Algorithm::Histogram, &input);
        let mut config = UnifiedDiffConfig::default();
        if complete {
            config.context_len(input.before.len().max(input.after.len()) as u32);
        }
        let patch = diff
            .unified_diff(&BasicLineDiffPrinter(&input.interner), config, &input)
            .to_string();
        if patch.is_empty() {
            self.push(Role::Note, "内容相同");
            if complete {
                for line in before.split_terminator('\n') {
                    self.push(Role::Text, &format!(" {}", visible_code(line)));
                }
            }
        } else {
            for line in patch.split_terminator('\n') {
                let role = match line.as_bytes().first() {
                    Some(b'+') => Role::Added,
                    Some(b'-') => Role::Removed,
                    Some(b'@') => Role::Note,
                    _ => Role::Text,
                };
                self.push(role, &visible_code(line));
            }
        }
        if !before.is_empty() && !before.ends_with('\n') {
            self.push(Role::Note, "原文末尾无换行");
        }
        if !after.is_empty() && !after.ends_with('\n') {
            self.push(Role::Note, "候选末尾无换行");
        }
    }
}

fn visible_code(line: &str) -> String {
    display(&line.replace('\t', "⇥").replace('\r', "␍"))
}

pub(in crate::ui) struct ReviewText {
    text: PreviewText,
    roles: Vec<Role>,
}

impl ReviewText {
    pub(in crate::ui) fn render(
        &mut self,
        area: Rect,
        surface: &mut Buffer,
        theme: &Theme,
        scroll: &mut usize,
    ) {
        let base = theme.get("ui.popup");
        self.text.render_styled(area, surface, scroll, |line| {
            let scope = match self.roles.get(line) {
                None | Some(Role::Text) => return base,
                Some(Role::Heading) => "markup.heading",
                Some(Role::Note) => "ui.text.info",
                Some(Role::Added) => "diff.plus",
                Some(Role::Removed) => "diff.minus",
                Some(Role::Success) => "diff.plus",
                Some(Role::Failure) => "error",
            };
            base.patch(theme.get(scope))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_diff_keeps_unchanged_lines_out_of_edit_and_marks_whitespace() {
        let mut lines = ReviewLines::default();
        lines.diff(
            "head\n\told\r\nkeep\na\nb\nc\nd\ne\nf\ng\nh\ntail",
            "head\n\tnew\r\nkeep\na\nb\nc\nd\ne\nf\ng\nh\nend\n",
            false,
        );
        assert!(lines.source.contains("-⇥old␍\n+⇥new␍"));
        assert!(!lines.source.contains("-keep") && !lines.source.contains("+keep"));
        assert_eq!(
            lines
                .source
                .lines()
                .filter(|line| line.starts_with("@@"))
                .count(),
            2
        );
        assert!(lines.source.contains("原文末尾无换行"));
        assert!(!lines.source.contains("候选末尾无换行"));
    }

    #[test]
    fn result_diff_for_merge_preserves_complete_context_including_unchanged_candidate() {
        let before = "first\nkeep\nkeep\nkeep\nkeep\nkeep\nold\nlast\n";
        let after = before.replace("old", "new");
        let mut lines = ReviewLines::default();
        lines.diff(before, &after, true);
        assert!(lines.source.contains(" first\n") && lines.source.contains("-old\n+new\n last"));
        assert_eq!(lines.source.matches(" keep\n").count(), 5);
        let mut unchanged = ReviewLines::default();
        unchanged.diff(before, before, true);
        assert!(
            unchanged.source.contains("内容相同\n first") && unchanged.source.contains(" last")
        );
    }
}
