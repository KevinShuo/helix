# Executable AI completion checks

`ai-completion-eval-fixtures.json` contains 30 Python/C++ cursor holes, reference
insertions and executable checks. The development/holdout assignment is fixed.
This is a targeted regression set, not a general accuracy benchmark.
`ai-completion-contract-fixtures.json` adds eight long-function cases whose
behavioral contract lies outside the nearby prefix. Pass it through `--fixtures`
and `HELIX_TEST_AI_FIXTURES` to compare contract recovery separately. The C++ scorer
provides a standard-header prelude; passing a semantic check does not prove that
the insertion's dependencies are declared by the original source file.

The scorer needs Python 3.11+ and clang++ or g++ on PATH. From the repository root:

```powershell
python contrib/ai-completion-eval.py --reference
$env:HELIX_DISABLE_AUTO_GRAMMAR_BUILD = '1'
$env:HELIX_RUNTIME = "$PWD/target/release/runtime"
$env:HELIX_TEST_AI_ENDPOINT = 'http://127.0.0.1:19240/v1/chat/completions'
$env:HELIX_TEST_AI_MODEL = 'Qwen3.5-9B-Q6_K'
$env:HELIX_TEST_AI_PROMPT_MODE = 'gap'
$env:HELIX_TEST_AI_FIXTURES = "$PWD/contrib/ai-completion-eval-fixtures.json"
$env:HELIX_TEST_AI_REPORT = "$PWD/target/ai-eval-report.json"
$env:HELIX_TEST_AI_SPLIT = 'all'
cargo test --locked -p helix-term --lib ai_completion_live_semantic_fixture_report -- --ignored --nocapture
python contrib/ai-completion-eval.py --report target/ai-eval-report.json
python -m unittest discover -s contrib -p test_ai_completion_eval.py
```

The Rust reporter uses the actual request, context-budget and output-validation
pipeline. It does not send reference insertions or checks to the model. Timings
exclude editor debounce/model startup. Compare providers using identical fixtures.
These fixtures exercise source context and expression gaps. The separate live
grounded-context and editor tests cover authoritative member metadata, where
conservative gap mode deliberately uses strict line prompting.

The scorer separates passing, wrong, withheld and backend-error results; missing
or duplicate results are rejected. Errors exit nonzero; wrong completions remain
scored results. C++ arithmetic wraps during evaluation so overflowing expressions
fail checks consistently rather than depending on optimizer behavior. Assertions
return failure codes instead of opening Windows dialogs. Generated programs have
a three-second timeout. Withheld suggestions never count as correct ones.
