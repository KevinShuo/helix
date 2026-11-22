//! A manual insertion stays attached to one unchanged cursor until accepted.
use crate::{document::Mode, Document, ViewId};
use helix_core::{text_annotations::InlineAnnotation, Range};
use helix_event::TaskController;

pub struct Operation {
    pub view: ViewId,
    pub version: i32,
    pub mode: Mode,
    pub selection: Range,
    pub cursor: usize,
    pub instruction: String,
    pub previous_draft: Option<String>,
    pub annotation: Option<InlineAnnotation>,
    pub failed: bool,
    pub controller: TaskController,
}

impl Operation {
    pub fn matches(&self, doc: &Document, view: ViewId) -> bool {
        self.view == view
            && self.version == doc.version()
            && !doc.readonly
            && doc.active_snippet.is_none()
            && doc.selection(view).len() == 1
            && doc.selection(view).primary() == self.selection
    }
}

impl Document {
    pub fn ai_inline_preview(&self, view: ViewId) -> Option<&InlineAnnotation> {
        if let Some(op) = &self.ai_generate {
            return op
                .matches(self, view)
                .then_some(op.annotation.as_ref())
                .flatten();
        }
        self.ai_completion
            .as_ref()
            .filter(|op| op.is_valid(self, view))
            .map(|op| &op.annotation)
    }
}
