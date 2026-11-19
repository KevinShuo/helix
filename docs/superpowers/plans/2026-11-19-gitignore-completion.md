# .gitignore completion implementation plan

Implement the first release in `docs/superpowers/specs/2026-11-19-gitignore-completion-design.md` using the existing Rust completion pipeline, template catalog and Git-compatible rule matching.

- [x] Add regression fixtures for rule precedence, tracked files and unavailable Git metadata. Implement a background rule evaluator that overlays the edited line and reports its effect on a sample path.
- [x] Test and implement bounded directory sampling, project/template discovery, segment parsing, escaping, ranking, merging and cache refresh. Expose candidates with a replacement, filter key, detail and relative ranking.
- [x] Route insert-mode manual and automatic requests by filename and line context. Use captured char offsets and savepoints for the edit. Preserve independent LSP completion, document settings and multi-cursor consistency.
- [x] Exercise the feature through editor events, including selection, cancellation, undo and stale requests. Run focused and regression suites, formatting and an independent review.

Check nested project roots, Git worktrees, inaccessible data, punctuation triggers and mid-segment Unicode edits. Limit each sampled directory to 4096 entries and return at most 8 empty-line or 50 nonempty candidates.
