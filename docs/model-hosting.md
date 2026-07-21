# Model hosting — bring your own local model

Endora is **model-agnostic**. It talks to a local, OpenAI-compatible chat endpoint
over HTTP; it does **not** bundle, install, or manage a model runtime. You host the
model however you like, and point Endora at it with one setting.

That boundary is deliberate ([ADR 0008](adr/0008-local-model-adapter.md),
[ADR 0009](adr/0009-node-served-ui-and-single-container.md)): the model runs as its
own service, Endora as its own container, and the only thing between them is a URL.
Swap models, resize hardware, or move the endpoint to another host — change the URL,
nothing else. **This page is guidance, not a requirement. Where Endora's
responsibility ends is that URL.**

## How Endora connects

Two environment variables (see `docker-compose.yml`):

| Variable | What it is | Default |
| --- | --- | --- |
| `ENDORA_MODEL_URL` | An OpenAI-compatible base URL (`/v1`) | `http://localhost:11434/v1` |
| `ENDORA_MODEL` | The model name to request | `qwen2.5:7b` |

If the endpoint is unreachable or returns something unusable, the butler falls back
to a deterministic **scripted** brain, so the app never hard-fails on a missing
model. Endora's safety guardrails (skill routing, honest "can't-serve" replies, the
autonomy envelope, the egress guard) are deterministic and hold **regardless of the
model** — a weaker model degrades *quality*, not safety.

Any OpenAI-compatible server works: **Ollama**, **LM Studio**, **llama.cpp server**,
**vLLM**, **LocalAI**, etc. The examples below use Ollama.

## Model recommendations

What matters for the butler: **instruction-following**, **clean JSON**, and
**tool/skill selection**. Bigger is better, but a good 7–8B is the practical sweet
spot on consumer GPUs.

| Tier | Model (Ollama tag) | Why |
| --- | --- | --- |
| **Recommended default** | `qwen2.5:7b` | Strong instructions + JSON + tool use; fits ~8–12 GB. **Tested** (see below). |
| Sharper, if you have VRAM | `qwen2.5:14b` | Noticeably better reasoning; needs more VRAM/slower. |
| Alternatives | `llama3.1:8b`, `gemma2:9b`, `mistral-nemo:12b` | Solid instruction-followers. |
| Small / CPU / fallback | `qwen2.5:3b`, `llama3.2:3b` | Runs on little/no GPU, but weaker — more fabrication (the guardrails still hold; quality drops). |
| Vision (for the **Image review** skill) | `moondream`, `llava`, `llama3.2-vision` | Set as that skill's `model` setting in the Skills view. |

## Hardware guidance

Rough VRAM for a 4-bit (`q4`) quant, **plus ~1–3 GB** for context/KV cache. Higher
quants (q5/q8) and longer context need more.

| VRAM | Comfortable | Notes |
| --- | --- | --- |
| CPU-only | `*:3b` | Works but slow; expect the scripted fallback under load. |
| 6 GB | `*:3b` (7B is tight) | Entry GPUs. |
| **8–12 GB** | **`*:7b–9b`** | The sweet spot; 14B is tight. |
| 16 GB | `*:14b` | Comfortable; some 24–32B at low quant. |
| 24 GB+ | `*:32b` | Headroom for larger models / longer context. |

Approx model footprints (q4): 3B ≈ 2 GB · 7–8B ≈ 4.5–5 GB · 9B ≈ 5.5 GB ·
14B ≈ 9 GB · 32B ≈ 20 GB · 70B ≈ 40 GB+.

## Tested setups

Configurations actually verified with Endora (community-tested entries welcome via
PR):

| Host | GPU | Runtime | Model | Result |
| --- | --- | --- | --- | --- |
| Home NAS (Ubuntu) | NVIDIA RTX A2000 **12 GB** | Ollama (Docker, `:11434`) | `qwen2.5:7b` | ✅ Snappy; reliable tool selection, clean JSON, faithful result relaying. `moondream` for image review on the same box. |

The `llama3.2:3b` we started with technically ran on the same hardware but was too
weak (fabricated facts, poor tool selection) — a good illustration of why the 7B
tier is the recommended floor for a good experience.

## Example: Ollama alongside Endora

Ollama as its own service (a container or a host install), with Endora pointed at it:

```bash
# On the model host:
ollama pull qwen2.5:7b
ollama pull moondream          # optional, for the Image review skill

# Endora (docker-compose.yml):
#   ENDORA_MODEL_URL: "http://host.docker.internal:11434/v1"   # or http://ollama:11434/v1 on a shared network
#   ENDORA_MODEL: "qwen2.5:7b"
```

Keep Ollama a **separate** service from Endora (do not co-bundle) — that's what keeps
the two loosely coupled and each highly cohesive. Endora only needs the URL.

## Security note

An OpenAI-compatible server like Ollama typically listens on `:11434` **without
auth** — anything on your LAN that can reach the port can use the model. That's
normal for a home lab, but if you want it locked down, bind it to localhost, put it
on an internal Docker network, or front it with auth. Endora reaching it by URL works
in any of those arrangements.
