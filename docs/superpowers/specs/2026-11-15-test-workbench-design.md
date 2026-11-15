# Native test workbench

Approved scope: the user selected the native workbench proposal and explicitly
asked to implement gutter run buttons, a results panel and failed-test reruns.
Python/pytest (including unittest.TestCase) and C/C++/GoogleTest are in v1;
debugging, coverage and automatic reruns are v2.

## Approaches

A gutter shortcut to the existing AI test log would be cheaper but cannot manage
handwritten tests, individual outcomes or failed-only reruns. The selected native
workbench has its own discovery, execution and state; AI generation remains an
independent producer of ordinary test files.

## Interaction

Add a configurable `test` gutter independent of fold and diagnostics/breakpoints.
Tree-sitter identifies Python test functions/classes and GoogleTest macros without
executing project code on document open. A button runs that declaration; cursor,
file and project commands provide keyboard equivalents. Parameterized declarations
select all collected instances, while the results panel can rerun a single instance.
Empty test gutter cells do nothing. Run symbols become result symbols after a run;
editing any open file in the project makes previous results stale.

The docked results panel shows expandable file/suite groups and individual cases,
status, duration, selected-case details and output. Enter jumps to a failure or test
location; `r` reruns selection, `f` reruns failures, `R` reruns the previous scope,
`s` stops, Tab switches details/tree, Esc returns editing focus, q closes the panel.
Closing the panel does not stop the task; `:test-show` reopens it. Mouse selection
and toolbar actions accompany keyboard controls. No DAP changes in v1.

## Execution

Editor-owned workbench state is independent of AiTest Operation/provider settings.
Use one cancellable operation at a time and fence asynchronous updates by run ID.
Keep explicit statuses queued/running/passed/failed/skipped/error/cancelled, and
separate configure/build/collection failures from assertion failures. Zero selected
tests is an error, never a passing run. Skips and xfail are visible separately.
Reject runs while project buffers are unsaved; never run old disk contents silently.
Bound logs and timeouts, use argv rather than shell text, and reuse owned-process
tree cancellation from the AI test runner. No LLM calls for discovery/execution.

pytest uses a bundled, embedded helper with official hooks. JSONL events written
to a private temporary report file carry collected nodeids/locations and completed
setup/call/teardown outcomes. Rust updates the tree while the process runs and
retains stdout/stderr for collection/crash failures. Never depend on colored pytest
console formatting. Use the configured interpreter/project venv discovery.

CMake configures/builds incrementally, respecting existing build directory, preset,
configuration, toolchain and target settings. CTest --show-only=json-v1 discovers
registered commands. GoogleTest --gtest_filter identities are reconciled with source
declarations (including parameterized names). Execute selected CTest entries using
exact escaped names, preserve CTest environment/working-directory settings, and
read CTest JUnit reports for skips/failures/timing. If a CTest entry groups a whole
executable, expose it as such rather than pretending it is individually runnable.
Projects without registered tests receive an actionable error. Ambiguous duplicate
CTest names are rejected before execution; projects
with overlapping suites across executables need distinct TEST_PREFIX values.
Running handwritten tests never rewrites CMakeLists.txt or installs dependencies. Existing AI generation
keeps its installed/vcpkg GTest first, FetchContent fallback integration policy.

General test settings live in `[editor.test]`, initially inheriting unspecified
Python/CMake/run-timeout values from `[editor.ai-test]` for compatibility. The gutter
is explicitly enabled in the default layout and can be removed like other gutters.
Use explicit test commands/clicks as execution authorization, matching :compile
and :ai-test-run; the repository has no generic TrustQuery for test execution.
Opening the test panel closes the Git dock and vice versa in v1.

## Verification

Pin source discovery (comments/nested helpers/decorators/macros), exact case
selection, parameterized mapping, stale results, teardown errors, skipped tests,
empty collection, cancellation and late callbacks. Real pytest smoke tests must
prove a failed-only rerun excludes a passing case. Real CMake/GTest smoke tests
must prove single-case selection, failed-only reruns, skip handling and build errors.
Editor integration tests verify button click never toggles a breakpoint, folded /
wrapped rows, keyboard panel focus, editor space reservation and failure navigation.
Run the repository's applicable Rust suites, formatting, and release build.

## Decisions

V1 runs a single language/project per invocation, inferred from the active file;
mixed-language workspaces can invoke each independently. Result history is the
latest run in this session, with stable IDs retained for reruns. A first C++ run may
need a full build before runtime discovery; ordinary opening/editing never builds.
