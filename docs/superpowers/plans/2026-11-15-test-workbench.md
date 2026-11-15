# Native Test Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run handwritten and generated Python/GoogleTest tests from the gutter,
inspect individual results and rerun failures in a docked Helix panel.

**Architecture:** Editor-owned state and static declaration locations feed a
framework-independent operation manager. pytest hook JSONL and CTest JSON/JUnit
adapters publish structured outcomes to a persistent terminal component.

**Tech Stack:** Rust, Tree-sitter, tokio, pytest hooks, CMake/CTest, existing TUI.

**Spec:** docs/superpowers/specs/2026-11-15-test-workbench-design.md

## Global Constraints

- Python/pytest (including unittest.TestCase) and C/C++/GoogleTest are in v1.
- No model calls, DAP changes, automatic reruns or coverage in v1.
- Do not modify unrelated dirty work or commit the shared checkout implicitly.
- Argv-only execution, bounded logs/timeouts, one fenced cancellable task.
- Existing CMakeLists.txt is read, not rewritten during ordinary test execution.

## Review Focus

- Parameter IDs and regex characters must select only intended tests.
- A teardown error overrides a successful call; skip/xfail are not passes.
- Empty gutter cells, folded/wrapped rows never toggle a breakpoint by accident.
- Cancelled/old operation callbacks cannot overwrite a subsequent run.
- Unsaved edits and changed source files invalidate previous passing indicators.

## Task 1: Declaration discovery and result state

Files: create helix-view/src/test.rs and test/discovery.rs; wire lib.rs/editor.rs.
Interfaces: TestConfig, TestState, Declaration, TestCase, Outcome, Scope; discover
accepts Document and returns declaration spans; selection uses stable runtime IDs.

- [x] Add failing tests for Python class/function/nested helper and C++ macros,
  parameterized name mapping, aggregate statuses and stale results.
- [x] Run focused helix-view tests and confirm the missing behavior.
- [x] Implement state and AST discovery; run focused tests to green.

## Task 2: Structured framework adapters and operation manager

Files: create helix-term/src/handlers/test.rs, test/python.rs, test/cmake.rs,
test/pytest_bridge.py; reuse ai_test/runner.rs process primitives.
Interfaces: start(editor, Scope), stop(editor), rerun_failed(editor), refresh;
framework adapters publish TestCase/phase/log changes fenced by operation ID.

- [x] Add failing selection/report tests including teardown, skip, empty collection,
  special IDs and stale operation updates; add real pytest/CMake smoke fixtures.
- [x] Implement pytest JSONL collection/reports and CTest JSON/JUnit exact selection.
- [x] Prove real failed-only reruns exclude passing cases and preserve CTest settings.

## Task 3: Gutter, commands and docked result panel

Files: gutter.rs/editor.rs in view; ui/editor.rs, ui/test.rs, commands/typed.rs,
application.rs and handlers.rs in term; integration tests/test/workbench.rs.
Interfaces: GutterType::Test; :test, :test-file, :test-project, :test-show,
:test-rerun, :test-rerun-failed, :test-stop; panel uses shared state.

- [x] Add failing editor integration tests for run click vs breakpoint, blank cells,
  folded/wrapped positions, focus, panel viewport and failure navigation.
- [x] Implement configurable gutter, commands and nonmodal result tree/details.
- [x] Verify close/reopen, mouse/keyboard toolbar actions and run cancellation.

## Task 4: Regression validation and delivery

Files: book/src/editor.md, configuration.md; validation record in docs/superpowers.

- [x] Document settings, prerequisites, commands, panel keys and CMake limitations.
- [x] Run focused and applicable full Rust suites, real framework tests and format.
- [x] Request an independent final review; address meaningful findings with tests.
- [x] Build release, verify startup/config and report paths, results and limitations.

## Ledger

Task 1: complete — declaration ID test observed failing before implementation;
focused view tests passed, including real Python/C++ syntax trees.
Pytest bridge: real tests observed failing with missing implementation, then 2
passed (7 mixed outcomes, parameter IDs, unittest, failed-only rerun).
Ruling: CTest entries without per-case gtest filters cannot satisfy a single-case
click; return an actionable error rather than silently widening scope. Preserve
the original RunRequest on rerun, and make Git/test docks mutually exclusive.

Review fixes: observed failing real pytest rootpath/deselection regressions, then
4 bridge tests passed. Observed failing CTest fixture and duplicate-name unit tests,
then 3 adapter tests passed. Added File API artifact/source disambiguation, absolute
pytest selectors, document/language-aware caches and decorator containment spans.
The first editor run exposed delayed :test-show focus and the first CTest editor
run exposed an unfocused existing panel on new runs; focus requests now live in
editor state and are consumed by the component / render refresh.

Task 2: complete — four real pytest bridge checks, CTest JSON/JUnit/File API unit
checks and actual CMake/GTest runs passed. Added a final guard after proving CTest
permits duplicate registered names; ambiguous selections fail before execution.
Task 3: complete — 3 editor tests and 3 explicit real-framework flows passed both
before and after the final guard. Ordinary units: 163 term + 91 view passed.
Broad editor regression: 334 passed / 17 ignored with four workers.
Ruling: reuse the already-tested folded/wrapped gutter projection API; dedicated
test-gutter regressions cover blank/folded rows, while existing view/minimap
softwrap and grapheme-coordinate suites verify the shared projection.
Ruling: preserve the current branch/working files without a merge/commit menu;
another user task is concurrently editing diagnostics in this shared checkout.
Ruling: the ordinary final ThinLTO link crashed with STATUS_HEAP_CORRUPTION.
A main-crate-only `-C lto=off` retry failed with LNK1136 against the existing LTO
dependency artifacts. Rebuild the entire release graph in a separate C: temporary
target directory with `CARGO_PROFILE_RELEASE_LTO=false` and incremental disabled.
Keep release optimization and repository profile defaults; the local binary has
an unmeasured possible size/performance tradeoff. All post-guard unit and six
workbench/editor/framework checks have already passed.
Task 4: complete — independent fresh dependency rebuild with release LTO disabled
succeeded (4m49s), final installed SHA matches staging, Python/C++ health and custom
gutter configuration checks passed. Original and intermediate binaries/config
were backed up; the active editor was preserved. Final evidence and limitations:
docs/superpowers/2026-11-15-test-workbench-validation.md.

Ruling: continue in the existing codex/ai-test-generation feature checkout because
it contains the prerequisite implementation and unrelated uncommitted provider
work. Preserve it and keep this change uncommitted. User explicitly approved the
recommended scope and requested execution; no additional approval handoff.
