# Class folding and declaration gutter

The user requested clickable fold symbols beside functions and classes, as in
the supplied PyCharm screenshots. This extends the earlier function-only scope.

Use the existing `fold` gutter and per-view fold state. Enable the gutter in the
user's configuration. Place symbols on the declaration's first line (after any
decorators/template wrapper), independent of where a multiline signature ends.
An expanded definition shows `▾`, a closed definition `▸`; clicking that symbol
toggles only its definition. Blank gutter rows do nothing.

Add dedicated `fold.class` captures for Python classes, C++ classes and C/C++
structs. Preserve the full header, C/C++ closing braces and suffixes. Keep empty,
single-line and incomplete definitions excluded. Nested method folds survive
closing/opening their class; edits and cursor protection use existing fold state.
Docstrings and arbitrary blocks remain outside this extension.

- [x] Reproduce missing class captures and declaration-line arrows with real
  syntax and mouse integration tests.
- [x] Extend candidate queries and gutter hit-testing; update command text/docs.
- [x] Run core/view/term checks and an independent review; fix relevant findings.
- [x] Build, verify and install the updated release; preserve original local edits
  and record publication/rollout according to the user's standing instructions.
