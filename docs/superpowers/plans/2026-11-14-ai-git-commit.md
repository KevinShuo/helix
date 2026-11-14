# AI Git Commit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox syntax for tracking.

**Goal:** Ship native AI file recommendations and user-confirmed staging/Conventional Commits with Chinese/English output, then build, merge, publish and install.
**Architecture:** Extend the existing Git backend with immutable commit snapshots and guarded writes. Add independent bounded AI analysis and a native commit workspace sharing the Git panel infrastructure.
**Tech Stack:** Rust, Tokio, Git executable, existing OpenAI-compatible transport and terminal components; no new dependency.
**Spec:** docs/superpowers/specs/2026-11-14-ai-git-commit-design.md (approved by user).

## Global Constraints

- Default Chinese, selectable English; Conventional Commits; no automatic push in the product flow.
- AI recommendations never write index; explicit stage and explicit commit are the two mutation actions.
- Preserve existing and partial staged contents unless user explicitly selects replacement/unstage.
- Snapshot validation, workspace trust, exact path arguments, hooks/signing respected, bounded input and stale callback rejection.
- All mutation tests use temporary repositories. Keep git-disabled builds working and existing Space g / Space v bindings.
- User authorized implementation, release build, master merge/push, GitHub release and installed binary replacement in this turn; no additional permission gate.

## Review Focus

- External index/HEAD edits during asynchronous requests must not commit unexpected content.
- Partial staging and rename/deletion paths must preserve unrelated index entries.
- Failed or mutating hooks and uncertain exit must not lead to duplicate commits.
- Manual message edits and language switches must survive late AI callbacks.
- Sensitive/unsupported/oversized diff must never be silently presented as fully analyzed.

## Task 1: Git snapshot and write backend

**Files:** create `helix-vcs/src/log/commit.rs`, `commit_test.rs`; modify `helix-vcs/src/log.rs` only for module wiring.
**Interfaces:** exported `CommitSnapshot { head: Option<String>, head_name: String, files: Vec<CommitFile>, ..private }`; `CommitFile { id: usize, path: PathBuf, old_path: Option<PathBuf>, staged: bool, unstaged: bool, untracked: bool, partial: bool, staged_diff: String, worktree_diff: String, ai_eligible: bool, exclusion: Option<String> }`; `CommitOutcome { oid: String, message: String, changed_from_preview: bool }`.
Methods: `commit_snapshot()`, `stage_files(&CommitSnapshot, &[usize])`, `unstage_files(&CommitSnapshot, &[usize])` return `anyhow::Result<CommitSnapshot>`; `commit_staged(&CommitSnapshot, &str) -> anyhow::Result<CommitOutcome>`.

- [x] Write tests for nonmutating snapshots, staged-vs-worktree diffs, partial stage preservation, exact Unicode/spaced paths and unborn repositories. Run and observe missing implementation failure.
- [x] Implement bounded snapshots and guarded path-level writes; stable pre/post snapshots; manual eligibility metadata for binary/sensitive/oversize inputs.
- [x] Add stale HEAD/index/content, rename/delete, failed and modifying hook, empty commit and retry tests. Run `cargo test -p helix-vcs commit` and require all new tests pass.

## Task 2: Independent configuration and AI client

**Files:** create `helix-view/src/ai_commit.rs`, `helix-term/src/handlers/ai_commit.rs` and focused child modules if needed; wire `helix-view/src/lib.rs`, `editor.rs`, `helix-term/src/handlers.rs`.
**Interfaces:** `AiCommitConfig`, `CommitLanguage::{Chinese,English}`; client input `AiFileInput { id: usize, path: String, staged: bool, partial: bool, diff: String, eligible: bool }`; `Recommendation { selected: Vec<usize>, reasons: BTreeMap<usize,String>, incomplete: bool }`; async `recommend(model,config,files,intent)` and `generate_message(model,config,files,intent) -> Result<GeneratedMessage { text: String, incomplete: bool }>`; `validate_message(&str) -> Result<()>`.

- [x] Write request/response/format tests before production implementation; observe RED for missing APIs.
- [x] Implement independent config (disabled, zh-CN, 30000ms,768 output tokens,4096 input bytes,64 files), explicit bounded serial batches, structured IDs, actual model context budgeting and shared transport/readiness.
- [x] Test Chinese/English, invalid IDs/types/JSON, skipped and overbudget inputs, no false complete message. Run `cargo test -p helix-term --lib ai_commit` and relevant view tests.

## Task 3: Native commit workspace and integration

**Files:** create `helix-term/src/ui/git/commit.rs` and children; modify Git menu/render, typed commands, shared explicit-AI arbitration and editor config integration as needed.
**Interfaces:** `commit::open(&mut compositor::Context, intent: String) -> Result<()>`, `commit::cancel(&mut compositor::Context)`; component id `ai-commit`; session-bound async callbacks and draft revisions.

- [x] Add UI state tests first: partial stage default false, stale callbacks ignored, manual drafts retained, language switching non-destructive, commit blocked for stale/incomplete drafts. Observe RED.
- [x] Build docking/fullscreen workspace, file/staged/diff/message navigation, explicit stage/unstage/generate/commit actions, full manual fallback and existing Git menu entry.
- [x] Add trusted workspace and related dirty buffer checks, exact final staged preview, background mutation lifecycle, one explicit AI generation at a time, and cancellation limited to read/AI phases.
- [x] Run focused unit and temporary-repo integration tests; update book command/config/usage documentation and design approval status.

## Task 4: Review and verification

- [ ] Run formatting, package tests and workspace tests with logged output. Compare any existing failures with prior validation report and name them honestly.
- [ ] Check git-disabled builds. Independent whole-feature reviewer checks spec and Review Focus; address material findings with regression tests.
- [ ] Test native workflow with mock model and actual configured local model in temporary repositories; Chinese/English and actual commit result, message editing, cancel/stage/commit.
- [ ] Record commands/results/limitations in `docs/superpowers/2026-11-14-ai-git-commit-validation.md`, commit implementation.

## Task 5: Authorized delivery

- [ ] Build `cargo build -j 1 --release -p helix-term --bin hx --target-dir target/publish-build`; verify binary and runtime.
- [ ] Fetch origin/master, merge safely with tested tree, push master/feature and unique annotated `ai-commit-2026.11.14` tag without force.
- [ ] Package Windows x64 executable, tracked runtime, 302 grammar DLLs, license, Chinese README, example configuration and build metadata. Isolated extraction smoke and SHA256 verification.
- [ ] Publish GitHub release using existing credential-manager API helpers; verify public/latest state and all three asset digests.
- [ ] Back up installed binary/config; enable ai-commit Chinese, replace binary without killing active user session; verify installed version, hash and feature. Keep old backup and report release link.

## Execution record

Use current clean feature checkout (only pre-existing untracked `.idea/`); no new worktree is needed. Backend and AI client have disjoint file ownership and fixed interfaces and can run in parallel under dispatching-parallel-agents; UI integration is the parent task after those interfaces exist. Build/test commands serialize on Cargo's shared target lock. The current user explicitly authorized end-to-end execution, overriding the skill's default plan approval handoff.
