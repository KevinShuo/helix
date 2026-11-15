# AI test generation

User-approved scope: explicit current-function test generation for C/C++ with
GoogleTest and Python with pytest, preview before application, optional execution,
and one bounded repair for generated-test compile/link/import failures. CMake
configuration/environment failures retain diagnostics for manual action. CMake changes are part
of the same preview. Prefer existing/local/vcpkg GTest; permit pinned FetchContent
only if GTest is unavailable. Reuse named providers and on-demand local 27B with
bounded thinking; global DeepSeek switching remains effective.

## Approaches

Discussed generation-only, generation plus validation and one repair (selected),
and a repository-wide agent. The selected approach provides executable tests
without an open-ended agent or automatic business-code restructuring.

## Behavior

- `:ai-test` captures the current function, whole-file source, relevant headers,
  existing tests/fixtures and project configuration within bounded context.
- Preview includes the exact changed files and assumptions. Enter applies changes
  to editable buffers; the user saves with `:wa!` when new directories are needed.
  `:ai-test-run` requires saved source and generated files and executes only
  application-selected commands. `:ai-test-cancel` cancels generation/execution.
- Source and supporting-file snapshots are checked before applying; dirty buffers
  are never silently overwritten. Each file change is undoable in its buffer.
- Generation creates an independently named test file per function, avoiding
  replacement of pre-existing handwritten tests. Regeneration appends no duplicates.
- CMake discovery supports literal add_library/add_executable/target_sources and
  test targets linked to GTest. Ambiguous/custom layouts require explicit config
  rather than guessed executable splitting. Link existing libraries whenever
  possible. Non-CMake projects receive drafts without automatic build integration.
  C sources retain C compilation.
- New CMake integration lives under a dedicated tests/helix-ai directory, with
  root CTest enablement and a guarded subdirectory. Existing test targets can be
  selected explicitly. Existing project options, presets and dependencies remain.
- Resolve GTest from existing targets, then find_package(GTest CONFIG QUIET), then
  the ordinary FindGTest module; finally allow pinned FetchContent. Pass configured
  toolchain/preset or VCPKG_ROOT toolchain when configuring a new build tree.
- Python uses configured or project virtual-environment Python and pytest;
  no automatic package installation or path injection. Missing environments remain
  actionable diagnostics. Complex async/plugin/hardware cases are draft-only.
- Model response contains test code and concise assumptions, never commands or
  arbitrary output paths. Validate complete response and language syntax; reject
  skipped/xfail/disabled tests and empty assertion sets.
- Runner separates configure/build/environment/assertion failures. Only explicit
  generated-test compile/link/import failures permit one repair; repairs affect generated
  tests only and return to preview, never rewrite assertions just to get green.
  No automatic source edits or cloud fallback. Commands use argv, bounded logs,
  timeout and process-tree cleanup on cancellation.

## Implementation calls

Add AiFeature::Test and an independent AiTestConfig. Keep implementation in
helix-term/src/handlers/ai_test/{project,client,runner}. Shared state lives in
helix-view/src/ai_test.rs; preview in helix-term/src/ui/ai_test.rs. Reuse syntax
target extraction with a distinct mode accepting already documented functions.
Defaults: disabled, local-provider optional, 4096 final tokens, 120s generation,
180s runner stage timeout, FetchContent allowed, one repair. Test-generation
requests explicitly reserve final tokens plus reasoning and reject overflow.

## Validation

Configuration routing/local lifecycle regressions; C/C++/Python extraction with
existing docs; CMake installed-first/fallback and library target handling;
multi-file stale/dirty/duplicate protection; provider JSON and token handling;
real pytest and CMake/GTest smoke tests when installed; fake HTTP editor integration;
timeout/cancellation and assertion-failure no-repair tests; cargo test plus format
and diff checks. Hardware/provider benchmarks are opt-in, with no quality claims
derived from documentation/commit-generation scores.
