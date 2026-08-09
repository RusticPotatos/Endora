# 0008 — Local model adapter

## Status

Accepted (2026).

## Context

v1.0 uses a **real local model** as a reasoning component that *proposes* steps
in the learning loop, strictly behind the deterministic policy boundary
([ADR 0005](0005-models-propose-policy-authorizes.md)). Endora is local-first and
privacy-preserving and targets consumer hardware, prioritizing the majority over
high-end machines. We must choose how the node talks to a model, and which model,
without making either choice load-bearing.

## Decision

Integrate the model through a **replaceable adapter behind an application-defined
port**, talking to a **local, OpenAI-compatible HTTP endpoint**:

- The **port** (a trait for "given a prompt/context, return a proposal") is
  defined in `endora-application`. The **adapter** implementation lives in
  infrastructure. The Domain layer never references the model.
- The default runner is a **local OpenAI-compatible server** (e.g. Ollama or a
  llama.cpp server; MLX builds on Apple Silicon). Talking OpenAI-compatible HTTP
  keeps the adapter thin and lets local and (later, optional) cloud providers
  share one interface.
- **Default model: Qwen3.5 9B** (fits 8 GB VRAM / 16 GB Apple Silicon);
  **Qwen3.5 4B** as a lighter fallback for older machines. Larger models (e.g.
  Qwen3.6 27B) and cloud providers are post-1.0 adapters.
- **License gate:** before v1.0 ships, confirm the chosen model's weights license
  is compatible with distributing/recommending it from an Apache-2.0 project;
  record the finding here. The default pick is validated by a small eval on the
  loop's real prompts (proposing experiments, summarizing observations, drafting
  reflections).
- The model output is always a **proposal**; the node routes it through Policy &
  Consent before any consequential effect, and Audit records the outcome.

## Consequences

- Running the model out-of-process (its own local server) keeps heavy inference
  and its native dependencies out of the node binary and off the async executor.
- The specific model, quantization, and runner are swappable without touching
  domain or application code — only the adapter/config changes.
- Users must run a local model server; the node degrades gracefully (clear error,
  no crash) when none is reachable. First-run setup/docs must cover this.
- OpenAI-compatible does not mean identical across runners; the adapter must
  tolerate minor differences and pin expectations in tests.

## Alternatives considered

- **In-process inference** (bind llama.cpp, or run via `candle`/`burn` in Rust) —
  rejected for v1.0: pulls heavy/native model dependencies into the node and
  complicates builds; revisit if a single-binary UX becomes a priority.
- **A cloud provider as the v1.0 default** — rejected: violates local-first and
  privacy-by-architecture; cloud stays an optional, replaceable adapter.
- **A model-specific SDK instead of OpenAI-compatible HTTP** — rejected: couples
  us to one vendor's client; the compatible HTTP surface is the portable choice.
- **A stub/echo model for v1.0** — rejected per roadmap: v1.0 ships real local
  reasoning, not a placeholder.
