#[derive(Default, Debug)]
pub(super) struct Draft {
    pub text: String,
    pub cursor: usize,
    pub revision: u64,
    pub manual: bool,
    pub reviewed: bool,
    needs_review: bool,
}

impl Draft {
    pub fn apply_generated(&mut self, revision: u64, text: String, incomplete: bool) -> bool {
        if revision != self.revision {
            return false;
        }
        self.text = text;
        self.cursor = self.text.len();
        self.manual = false;
        self.reviewed = !incomplete;
        self.needs_review = incomplete;
        self.revision += 1;
        true
    }
    pub fn invalidate(&mut self) {
        self.reviewed = false;
        self.needs_review = !self.text.trim().is_empty();
        self.revision += 1;
    }
    pub fn keep_manual(&mut self) {
        self.manual = true;
        self.reviewed = true;
        self.needs_review = false;
        self.revision += 1;
    }
    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.edited();
    }
    pub fn left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i);
    }
    pub fn right(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        if end != self.cursor {
            self.text.drain(self.cursor..end);
            self.edited();
        }
    }
    pub fn delete(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.text.drain(self.cursor..self.cursor + c.len_utf8());
            self.edited();
        }
    }
    fn edited(&mut self) {
        self.manual = true;
        self.reviewed = !self.needs_review;
        self.revision += 1;
    }
    pub fn vertical(&mut self, down: bool) {
        let start = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
        let column = self.text[start..self.cursor].chars().count();
        let range = if down {
            let Some(end) = self.text[self.cursor..].find('\n') else {
                return;
            };
            let next = self.cursor + end + 1;
            next..self.text[next..]
                .find('\n')
                .map_or(self.text.len(), |i| next + i)
        } else {
            if start == 0 {
                return;
            }
            self.text[..start - 1].rfind('\n').map_or(0, |i| i + 1)..start - 1
        };
        self.cursor = self.text[range.clone()]
            .char_indices()
            .nth(column)
            .map_or(range.end, |(i, _)| range.start + i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_git_draft_late_generation_cannot_overwrite_manual_text() {
        let mut draft = Draft::default();
        let revision = draft.revision;
        draft.insert("fix(ai): 修复重试\n\n保留手动正文");
        assert!(!draft.apply_generated(revision, "feat(ai): old".into(), false));
        assert!(draft.text.contains("保留手动正文"));
        assert!(draft.manual);
    }

    #[test]
    fn ai_git_draft_scope_changes_require_explicit_review() {
        let mut draft = Draft::default();
        assert!(draft.apply_generated(0, "feat(ai): add docs".into(), false));
        assert!(draft.reviewed);
        draft.invalidate();
        assert!(!draft.reviewed);
        assert_eq!(draft.text, "feat(ai): add docs");
        draft.keep_manual();
        assert!(draft.manual && draft.reviewed);
    }

    #[test]
    fn ai_git_draft_unicode_edit_and_incomplete_review() {
        let mut draft = Draft::default();
        draft.apply_generated(0, "feat(ai): 中文".into(), true);
        assert!(!draft.reviewed);
        draft.left();
        draft.backspace();
        draft.insert("英");
        assert_eq!(draft.text, "feat(ai): 英文");
        draft.right();
        draft.insert("\n正文");
        assert!(draft.text.ends_with("\n正文"));
    }

    #[test]
    fn ai_git_draft_edits_do_not_clear_explicit_scope_or_incomplete_review() {
        let mut draft = Draft::default();
        draft.insert("fix(ai): manual");
        assert!(draft.reviewed);
        draft.invalidate();
        draft.insert(" edit");
        assert!(
            !draft.reviewed,
            "editing is not explicit approval of a changed staged range"
        );
        draft.keep_manual();
        assert!(draft.reviewed);
        let revision = draft.revision;
        draft.apply_generated(revision, "feat(ai): partial".into(), true);
        draft.backspace();
        assert!(
            !draft.reviewed,
            "incomplete analysis needs explicit manual retention"
        );
    }
}
