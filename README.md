# Endora

A local-first AI butler. It runs on **your** hardware, learns who you are over time,
and does useful things — weather, news, research, home context — while the model and
your data stay on your machine.

No cloud account. No surveillance. The language model *proposes*; deterministic policy
decides what actually happens — and it never does anything irreversible on its own.

## Why

- **Local AI.** The model runs on your box via Ollama (or any OpenAI-compatible
  server). Your conversation, memory, and data never leave it.
- **Normal hardware.** A 7B model on a consumer GPU is plenty — tested on an NVIDIA
  RTX A2000 12 GB with `qwen2.5:7b`. CPU works too, just slower.
- **You own it.** Everything is on disk, exportable and deletable. No accounts, no
  ads, no lock-in.
- **Safe by construction.** Skills sit behind a deterministic policy layer and an
  egress guard. It can research and draft on its own; it can't send, spend, or delete
  anything without you.

## What it does

- Chats naturally and builds a correctable **understanding** of you — beliefs with the
  evidence behind them, not a hidden profile.
- Uses real **skills** when it needs facts: weather, local news, active safety alerts,
  web search, Wikipedia, fetch a page, describe an image (local vision model), and read
  Home Assistant state to learn your routines. Keyless where possible; the rest you
  configure with a key or token, stored locally.
- **Proactive:** optional check-ins and a **daily brief** (weather / safety / news for
  where you're based).
- **Private egress:** an SSRF guard, an outbound secret tripwire, query minimization,
  and an optional proxy — so it can't be tricked into leaking data or reaching into
  your LAN.

## Quick start

Bring your own model — any OpenAI-compatible endpoint (Ollama, LM Studio, llama.cpp,
vLLM):

```bash
ollama pull qwen2.5:7b
make run-node                 # http://localhost:8787
```

Open **http://localhost:8787** — the node serves the whole UI; there's no separate app
to install. Or run it in a container (data persists in `./endora-data`):

```bash
make docker-build && make docker-run
```

- **Model recommendations, hardware guidance, tested setups, and private-egress /
  proxy notes:** [docs/model-hosting.md](docs/model-hosting.md).
- **Always-on and reaching it from your phone:** [docs/hosting.md](docs/hosting.md).
  The `0.x` API is unauthenticated, so keep it on a trusted network — see
  [SECURITY.md](SECURITY.md).

## The safety model

- **Models propose; policy authorizes.** The model is never the enforcement boundary.
- **Reversibility first.** It acts on its own only within reversible bounds — read,
  research, draft. Anything irreversible or outbound — send, spend, edit, delete — is
  blocked until you confirm. Some things can't be undone; it treats them that way.
- **Your data stays home** and is exportable and deletable; egress is guarded and
  minimized.

Full principles: [Constitution](docs/constitution.md). Every design decision is
recorded as an [ADR](docs/adr/README.md).

## How it's built

Rust, a domain-first modular monolith organized **by responsibility**, not by layer —
Responsibility-Oriented Clean Architecture (ROCA, [ADR 0026](docs/adr/0026-package-by-bounded-context.md)).
One authoritative backend (the **node**) holds all state and authority; clients are
thin. The model sits *behind* the policy boundary.

```text
clients ──HTTP/JSON──▶ node ──▶ policy boundary ──▶ skills / local SQLite
```

```text
app/      node (backend + web UI)   cli (thin client)   ← composition roots
domains/  understanding · capabilities · conversation · scheduling · platform
          each a crate layered domain / application / infrastructure, inward-pointing
shared/   kernel (ids, time, errors)   persistence (the shared SQLite handle)
```

See [docs/architecture.md](docs/architecture.md). There's also a CLI (`make run-cli
ARGS="health"`).

## Status

A working personal project, developed against a home server. It runs the butler, the
skills, the autonomy envelope, and the egress guard described above. Interfaces still
change and there's no tagged release yet — build one complete slice at a time.

## Contributing & license

Read [CONTRIBUTING.md](CONTRIBUTING.md), the [Code of Conduct](CODE_OF_CONDUCT.md), and
[SECURITY.md](SECURITY.md). **AI-assisted contributions must be flagged as such** in the
PR. Contributors branch from `develop`.

Licensed under [Apache-2.0](LICENSE). Copyright 2026 Endora contributors. See
[NOTICE](NOTICE).
