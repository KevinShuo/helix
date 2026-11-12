//! In-process merge drafts. A Document owns text and review state; this registry
//! owns only identities and the temporarily displaced native view tree.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context, Result};
use helix_vcs::{
    git_tools::{ConflictSupport, GitConflictData},
    merge::{MergeCandidate, MergeModel},
};

use crate::{graphics::Rect, tree::Tree, Document, DocumentId, Editor, View};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MergeSessionId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeLeave {
    KeepDraft,
    Discard,
}

#[derive(Debug)]
pub struct MergeDocumentState {
    pub session: MergeSessionId,
    pub data: GitConflictData,
    pub model: MergeModel,
    /// Text and history cannot change while an explicitly requested write runs.
    pub busy: bool,
}

struct Session {
    document: DocumentId,
    root: PathBuf,
    path: PathBuf,
    tree: Option<Tree>,
}

#[derive(Default)]
pub struct MergeSessions {
    next: u64,
    sessions: BTreeMap<MergeSessionId, Session>,
    active: Option<MergeSessionId>,
    saved_tree: Option<Tree>,
    restore_area: Option<Rect>,
}

impl MergeSessions {
    pub fn find(&self, root: &Path, path: &Path) -> Option<MergeSessionId> {
        self.sessions
            .iter()
            .find_map(|(&id, session)| (session.root == root && session.path == path).then_some(id))
    }

    pub fn document(&self, id: MergeSessionId) -> Option<DocumentId> {
        self.sessions.get(&id).map(|s| s.document)
    }

    pub fn iter(&self) -> impl Iterator<Item = (MergeSessionId, DocumentId)> + '_ {
        self.sessions
            .iter()
            .map(|(&id, session)| (id, session.document))
    }
}

impl Editor {
    pub fn active_merge(&self) -> Option<MergeSessionId> {
        self.merge_sessions.active
    }

    pub fn enter_merge(
        &mut self,
        data: GitConflictData,
        candidate: MergeCandidate,
    ) -> Result<MergeSessionId> {
        ensure!(
            data.support == ConflictSupport::Text,
            "This conflict requires an external merge tool"
        );
        if let Some(id) = self
            .merge_sessions
            .find(data.version.root(), &data.file.path)
        {
            self.resume_merge(id)?;
            return Ok(id);
        }
        let model = MergeModel::from_candidate(candidate);
        let mut doc = Document::from(
            model.initial_result().clone(),
            None,
            self.config.clone(),
            self.syn_loader.clone(),
        );
        let loader = self.syn_loader.load();
        let language = loader
            .language_for_filename(&data.file.path)
            .map(|lang| loader.language(lang).config().clone());
        doc.set_language(language, &loader);
        doc.detect_indent_and_line_ending();
        // The draft is normalized LF; the original format is applied only by Apply.
        doc.line_ending = helix_core::LineEnding::LF;
        self.merge_sessions.next += 1;
        let id = MergeSessionId(self.merge_sessions.next);
        let root = data.version.root().to_owned();
        let path = data.file.path.clone();
        doc.merge = Some(MergeDocumentState {
            session: id,
            data,
            model,
            busy: false,
        });
        let document = self.new_document(doc);
        self.merge_sessions.sessions.insert(
            id,
            Session {
                document,
                root,
                path,
                tree: None,
            },
        );
        self.resume_merge(id)?;
        Ok(id)
    }

    pub fn resume_merge(&mut self, id: MergeSessionId) -> Result<()> {
        if self.active_merge() == Some(id) {
            return Ok(());
        }
        ensure!(
            self.merge_sessions.sessions.contains_key(&id),
            "Merge draft no longer exists"
        );
        self.leave_merge(MergeLeave::KeepDraft)?;
        self.enter_normal_mode();
        if !self.tree.is_empty() {
            let (view, doc) = current!(self);
            doc.append_changes_to_history(view);
        }
        let area = self.tree.area();
        let session = self.merge_sessions.sessions.get_mut(&id).unwrap();
        let mut tree = session.tree.take().unwrap_or_else(|| {
            let mut tree = Tree::new(area);
            tree.insert(View::new(
                session.document,
                self.config.load().gutters.clone(),
            ));
            tree
        });
        tree.resize(area);
        let doc = self
            .documents
            .get_mut(&session.document)
            .context("Merge document no longer exists")?;
        doc.ensure_view_init(tree.focus);
        doc.mark_as_focused();
        self.merge_sessions.saved_tree = Some(std::mem::replace(&mut self.tree, tree));
        self.merge_sessions.restore_area = Some(area);
        self.merge_sessions.active = Some(id);
        Ok(())
    }

    /// The compositor supplies the complete editor area, before the merge panes
    /// reserve the center, so a resize also updates the tree being restored.
    pub fn merge_restore_area(&mut self, area: Rect) {
        if self.active_merge().is_some() {
            self.merge_sessions.restore_area = Some(area);
        }
    }

    pub fn leave_merge(&mut self, disposition: MergeLeave) -> Result<()> {
        let Some(id) = self.active_merge() else {
            return Ok(());
        };
        let doc_id = self
            .merge_sessions
            .document(id)
            .context("Merge draft no longer exists")?;
        if disposition == MergeLeave::Discard
            && self.documents[&doc_id].merge().is_some_and(|m| m.busy)
        {
            bail!("Merge result is being applied; keep the draft until it finishes");
        }
        self.enter_normal_mode();
        let (view, doc) = current!(self);
        doc.append_changes_to_history(view);
        let mut saved = self
            .merge_sessions
            .saved_tree
            .take()
            .context("Missing original editor layout")?;
        if let Some(area) = self.merge_sessions.restore_area.take() {
            saved.resize(area);
        }
        let draft_tree = std::mem::replace(&mut self.tree, saved);
        self.merge_sessions.active = None;
        self.merge_sessions.sessions.get_mut(&id).unwrap().tree = Some(draft_tree);
        // ViewIds belong to each Tree and may coincide. Never call close() on
        // temporary views: it would remove selections/folds from original docs.
        if disposition == MergeLeave::Discard {
            self.discard_merge(id)?;
        }
        Ok(())
    }

    pub fn discard_merge(&mut self, id: MergeSessionId) -> Result<()> {
        if self.active_merge() == Some(id) {
            return self.leave_merge(MergeLeave::Discard);
        }
        let Some(document) = self.merge_sessions.document(id) else {
            return Ok(());
        };
        ensure!(
            !self.documents[&document].merge().is_some_and(|m| m.busy),
            "Merge result is being applied"
        );
        self.merge_sessions.sessions.remove(&id);
        self.saves.remove(&document);
        if let Some(doc) = self.documents.remove(&document) {
            helix_event::dispatch(crate::events::DocumentDidClose { editor: self, doc });
        }
        Ok(())
    }
}
