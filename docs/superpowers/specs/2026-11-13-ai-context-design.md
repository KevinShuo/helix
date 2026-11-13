# Grounded AI completion context

The user requested further quality improvements after enabling conservative
completion. Continue directly with the existing local model and checkout.

Fixed character windows can omit enclosing parameters/import aliases, and the
model currently lacks the language server's real member names. A five-fixture
local-model spike reproduced invented parameter/field names; supplying the
missing declarations/names corrected all five outputs. This is exploratory
evidence, not a general accuracy estimate.

In conservative mode, recover missing enclosing function/class/impl signatures
and imports from the current syntax tree, then add bounded member candidates
from the already-connected completion language server for `.`, `->`, `::` and
partial member names. Fetch after debounce, wait at most 200ms, cancel when the
edit context changes, and fall back without interrupting editing.

Keep total prefix-source plus supplemental context within `prefix-chars`, with
at most 600 source-note characters and 600 LSP-note characters. Preserve the
exact typed current line; discard supplementary notes before cropping that line.
No project files are opened for this feature and no new model is downloaded.

Only export candidate labels/type details, not snippets, edits or commands.
Use complete small member lists to reject an invented first member name;
incomplete/oversized/unusable lists are hints only. Preserve existing popup,
accept/undo, stale-response and conservative-disabled behavior.

Regression tests, real local-model probes and an actual clangd/editor check
must establish that context is collected and useful. Report limits candidly.
