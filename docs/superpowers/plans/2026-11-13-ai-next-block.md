# AI next-line and block prediction

User goal: continue improving AI completion quality and add next-line/short-block
prediction. User requests retaining Shift+Right. Existing direct-execution
authorization applies; keep the current checkout, preferences, model and sessions.

Design: ordinary partial lines retain the existing conservative gap/member-line
path. An opt-in `predict-next` setting enables automatic bounded predictions on
an indented blank next line with usable context, or after a block opener. Return
one line or one short coherent block (default 6 lines, 192 tokens), never silently
clip a longer/truncated response. Use complete-line generation with explicit
indentation, extract only missing text, preserve suffix and line endings, and
reject repeated source, malformed indentation and suspicious output. Empty files,
ambiguous empty top-level lines and mid-line suffixes do not trigger prediction.
Shift+Right accepts the entire preview as one undo step; typing/moving/cancellation
uses the existing snapshot/controller behavior.

Quality: where a grammar is available, asynchronously parse a bounded document
snapshot. Reject completions introducing syntax errors into a previously valid
document; predictions must produce valid syntax. Do not claim semantic proof.
Keep parsing off the editor UI thread; skip expensive full-document checking for
oversized documents, and cancel stale results using the existing controller.

- [x] Configuration regression, then add opt-in predict-next/line/token settings.
- [x] HTTP/client regressions for next line, multi-line indentation, suffix,
  truncation, echoed source, excessive length and untouched inline behavior.
- [x] Implement prediction eligibility, prompts and full-insertion validation.
- [x] Add syntax-validation regressions and wire bounded background checking.
- [x] Real editor tests: auto request, gray preview, Shift+Right accept, undo,
  CRLF, existing suffix, continued typing/movement and disabled configuration.
- [x] Real local-model next-line and block predictions, existing semantic/source
  regressions, normal/config/editor tests, independent review and formatting.
- [x] Release build, runtime check, backup and enable prediction in the user's
  existing configuration. Preserve original editor/model processes; document use.

No commit, push or global Junie setting change. Normal completion limits remain
independent of the new bounded prediction settings.
