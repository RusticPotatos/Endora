# 0017 — Persona and voice

## Status

Accepted (2026). Implements [ADR 0014](0014-the-butler-conversation-values-attention.md) §4.

## Context

The butler can converse, propose, learn preferences, and stay candid (0.7). Two
pieces of its *character* remain: how it **sounds** (persona/style) and whether it
can be **spoken to and heard** (voice). [ADR 0014](0014-the-butler-conversation-values-attention.md)
§4 set the personality model — style is a taste preference; the butler mirrors the
person's register with a golden-rule floor; none of this touches the invariants
(honest, never sycophantic). This ADR makes persona and voice concrete for a first
build.

## Decision

### Style: mirror the register, with the floor — at the prompt

The butler's persona lives in its **system prompt**, not in code: it has a default
tone (candid, warm) and is told to **mirror the person's register** — matching
their formality, warmth, and politeness — **asymmetrically**: it reflects kindness
and register *upward* but **never** mirrors hostility, rudeness, or contempt
*downward*; it stays even and kind (the golden-rule floor). Explicit **style
preferences** already stored ("be terse", "be more formal") are honoured on top,
since they flow into the prompt via the preferences the butler is handed. Style
flavours *how* it speaks; it never softens *whether* it is truthful — the
anti-sycophancy invariant and its eval (§5) still bind.

Any product-facing **persona name** stays deliberately open (as in ADR 0014); the
character is carried by tone, not a label.

### Voice: the browser, client-side

Speech is a **client capability**, not a node feature — the node keeps speaking the
same text protocol. The web console uses the browser's built-in **Web Speech API**:

- **Text-to-speech** — `speechSynthesis` reads the butler's replies aloud, behind a
  toggle the person turns on.
- **Speech-to-text** — `SpeechRecognition` captures a spoken message into the chat
  input from a mic button.

No new dependency, no server-side audio, and it degrades cleanly: where the browser
lacks the API the controls simply do not appear, and typing still works.

**Two caveats, stated plainly.** (1) Some browsers' speech recognition streams audio
to a cloud service (e.g. Chrome), which crosses the local-first line — so voice is
**off by default and opt-in**. (2) Browsers only grant microphone access on a
**secure context** (HTTPS or `localhost`), so speaking *to* it over a plain-HTTP LAN
address is blocked by the browser; text-to-speech (it speaking to you) is not. The
console says so, and [docs/hosting.md](../hosting.md) documents giving the console an
HTTPS origin (e.g. `tailscale serve` or a TLS proxy). A fully local STT/TTS path (a
model on the node/host) is possible later behind the same client seam.

## Consequences

- The butler has a consistent character (candid, warm, register-mirroring) and can
  be spoken to and listened to — a real step toward "you live in the conversation."
- Zero backend change and zero new dependency: voice is browser-native and the
  protocol is unchanged, so any future client (native, mobile) can add voice its
  own way.
- Persona and style stay data/prompt-driven and correctable (via preferences),
  never hard-coded — consistent with the memory-rights and autonomy models.
- The privacy trade-off of browser STT is surfaced, not hidden; opt-in keeps the
  default local-first. A local speech path is deferred, not precluded.

## Alternatives considered

- **Server-side STT/TTS (a speech model on the node).** Rejected for now: heavy
  (another model, audio streaming, more deps) when the browser already does this
  for free. Revisit if a fully local voice path is wanted.
- **Bake the persona/tone into code.** Rejected: tone is a taste preference
  (ADR 0014) — it belongs in the prompt and in correctable preferences, not in
  compiled constants.
- **Commit to a persona name now.** Deferred: naming is a UX/branding call the
  person owns; the character does not depend on it.
- **Voice on by default.** Rejected: browser STT can leave the machine, so it must
  be opt-in to keep the local-first default honest.
