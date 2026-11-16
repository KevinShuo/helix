# Local inference performance

The user authorized optimizing local inference as far as practical. Preserve the
existing 9B completion model, 27B Q5 generation model, 64K generation context,
thinking choices, source privacy, and unrelated uncommitted work. Changes to
quantization or context are experimental alternatives, not silent substitutes.

## Approach

Measure the installed CUDA llama.cpp b11081 before changing runtimes. Compare
the current 56-layer F16 baseline against quantized KV caches with full GPU
offload, then MTP and n-gram speculation, CPU thread counts and batch settings.
Use identical synthetic prompts, record warm/cold latency, prompt/decode speed,
GPU memory and actual offloaded layers. Repeat finalists and check structured
output and executable generated code. Keep only demonstrated improvements.

Expose typed, validated provider `runtime` settings for KV cache precision,
speculation, draft length, threads and batch sizes. Defaults retain existing
behavior. Runtime changes restart the managed server; request-only changes do
not. Do not add arbitrary command-line injection or change remote providers.

Alternatives: smaller Q4 weights or shorter context may fit better but trade
precision/capacity; a newer upstream runtime may improve kernels but requires
separate comparison. Try these only after installed-runtime tuning is measured.

## Delivery

Reproducible benchmark script, raw observations and concise report; regression
tests and documentation; release executable and backed-up installed config with
the winning profiles. Preserve running editors and their unsaved buffers.
Do not claim an absolute hardware optimum: report tested alternatives and limits.
