// ---- Speakers: live mode (ADR 0104) ----------------------------------------------------------
// This device hears its speaker in one of two ways. LISTENING is an <audio> element playing the
// hub's MP3: the browser buffers seconds of it, which is what keeps it playing with the screen
// locked, and what makes it hopeless beside a television. LIVE is the hub's raw sound in 20 ms
// frames, fetched as a stream and played by an AudioWorklet with a small buffer of its own
// that grows when the network is rough and shrinks when it is calm: a fraction of a second.
//
// Live is chosen by itself when an AirPlay door is wired to this speaker (somebody is casting
// video or music from a device, and the picture is in front of you), or when the owner picks it.
// A page that goes to the background hands over to listening, because only a media element
// plays on with the screen locked, and takes live back when the page returns.
//
// Everything that decides something is a plain function with no outside names, so the console
// check can run it, and so the worklet can be built out of the very same text: the worklet's
// source is these functions' own `toString()` plus the processor below.

const LIVE_RATE = 44100;       // what the hub mixes at; the context is asked for it too
const LIVE_MIN = 300;          // the buffer's least depth, in ms, and
const LIVE_MAX = 800;          // its most
const LIVE_HEADER = 24;        // a frame's header, in bytes (see app/sound/src/live.rs)
const LIVE_TONE = { hz: 2000, amp: 0.1 };  // the delay probe's tone, which the hub plays

// ---- pure: the frame format ------------------------------------------------------------------
// Reads frames from a stream of chunks that cut them anywhere. Returns { push(chunk) -> frames,
// skipped }, where a frame is { flags, seq, atUs, rate, left, right } (samples as -1..1).
// Bytes that are not a frame are skipped to the next "EL", and counted.
function liveParser() {
  let held = new Uint8Array(0);
  const out = { skipped: 0, push: null };
  out.push = (chunk) => {
    if (chunk && chunk.length) {
      const joined = new Uint8Array(held.length + chunk.length);
      joined.set(held, 0); joined.set(chunk, held.length);
      held = joined;
    }
    const frames = [];
    let at = 0;
    for (;;) {
      if (held.length - at < 24) break;
      if (held[at] !== 0x45 || held[at + 1] !== 0x4c) { at += 1; out.skipped += 1; continue; }
      const view = new DataView(held.buffer, held.byteOffset + at, held.length - at);
      const bytes = view.getUint32(20, true);
      if (bytes > (1 << 20) || bytes % 2 !== 0) { at += 2; out.skipped += 2; continue; }
      if (held.length - at < 24 + bytes) break;
      const channels = view.getUint8(3) || 2;
      const n = Math.floor(bytes / 2 / channels);
      const left = new Float32Array(n), right = new Float32Array(n);
      for (let i = 0; i < n; i++) {
        left[i] = view.getInt16(24 + i * 2 * channels, true) / 32768;
        right[i] = channels > 1 ? view.getInt16(24 + i * 2 * channels + 2, true) / 32768 : left[i];
      }
      frames.push({
        flags: view.getUint8(2),
        seq: view.getUint32(4, true),
        atUs: view.getUint32(8, true) + view.getUint32(12, true) * 4294967296,
        rate: view.getUint32(16, true),
        left, right,
      });
      at += 24 + bytes;
    }
    held = held.slice(at);
    return frames;
  };
  return out;
}

// What the next frame's number says about the ones before it: how many are missing (the hub
// skipped them, or the network lost them), or that it is an old one arriving late. Numbers
// wrap at 2^32. `expected` is null for the first frame of a stream.
function liveSeqStep(expected, seq) {
  if (expected === null) return { missing: 0, stale: false };
  const d = (seq - expected) >>> 0;
  if (d === 0) return { missing: 0, stale: false };
  if (d < 0x80000000) return { missing: d, stale: false };
  return { missing: 0, stale: true };
}

// ---- pure: the adaptive buffer ---------------------------------------------------------------
// The target depth of the buffer, in ms, and how it moves. It starts at `min` (300), grows by
// a fixed step when the sound ran dry (+60) or frames went missing (+30), up to `max` (800), and
// shrinks by 10 ms for each 10 s that played without trouble, but never below a FLOOR: the
// target that last ran dry plus its step, which itself falls back towards `min` by 10 ms for
// each 30 s without a dry spell. So the buffer settles just above what the network needs, does
// not climb to a second and stay there (multiplying did), and does not shrink into the same
// dry spell every few minutes (shrinking to the minimum did).
// `ev` is { kind: "underrun" }, { kind: "gap" } or { kind: "tick", ms } (ms of smooth playing).
// Returns the new state; never changes the old. `floor` and `fcalm` may be missing (zero).
function liveJitter(s, ev) {
  const min = s.min, max = s.max;
  const floor = Math.max(min, Math.min(max, s.floor || 0));
  if (ev.kind === "underrun") {
    const target = Math.min(max, s.target + 60);
    return { target, min, max, calm: 0, floor: target, fcalm: 0 };
  }
  if (ev.kind === "gap") {
    return { target: Math.min(max, s.target + 30), min, max, calm: 0, floor, fcalm: s.fcalm || 0 };
  }
  if (ev.kind === "tick") {
    let calm = s.calm + ev.ms, fcalm = (s.fcalm || 0) + ev.ms, f = floor, target = s.target;
    while (fcalm >= 30000) { fcalm -= 30000; f = Math.max(min, f - 10); }
    while (calm >= 10000) { calm -= 10000; target = Math.max(min, f, target - 10); }
    return { target, min, max, calm, floor: f, fcalm };
  }
  return s;
}

// The state with a new minimum (lip-sync lowers or restores it): the target and the floor move
// by as much as the minimum did, and stay between the minimum and the maximum.
function liveRebase(s, min) {
  const d = min - s.min;
  const clamp = (x) => Math.max(min, Math.min(s.max, x));
  return { target: clamp(s.target + d), min, max: s.max, calm: s.calm || 0, floor: clamp((s.floor || 0) + d), fcalm: s.fcalm || 0 };
}

// What a lip-sync offset (ms; positive plays later, negative earlier) asks of the buffer.
// Later is a fixed extra delay on top of the target. Earlier lowers the buffer's minimum, but
// never under 80 ms; `short` says the offset asked for more than that allows.
function liveSyncPlan(ms, baseMin) {
  const sync = Math.max(-500, Math.min(1500, Math.round(Number(ms) || 0)));
  if (sync >= 0) return { extra: sync, min: baseMin, short: false };
  const want = baseMin + sync;
  return { extra: 0, min: Math.max(80, want), short: want < 80 };
}

// "+0.3 s", "−0.2 s": a lip-sync offset for the card.
function soundSyncLabel(ms) {
  const n = Math.round(Number(ms) || 0);
  return `${n < 0 ? "−" : "+"}${(Math.abs(n) / 1000).toFixed(1)} s`;
}

// How fast to play to bring the buffer to its target: half a percent faster when it holds more
// than 40 ms too much, two percent when it holds more than 100 ms too much (it would take half
// a minute otherwise), and the same either way for too little (a lip-sync nudge later is a
// buffer that must fill). (Half a percent is a tenth of a
// semitone; nobody hears it, and 5 ms of buffer goes every second. Two percent is a third of a
// semitone, for the few seconds it takes.)
function liveServo(depthMs, targetMs) {
  const err = depthMs - targetMs;
  if (err > 100) return 1.02;
  if (err > 40) return 1.005;
  if (err < -100) return 0.98;
  if (err < -40) return 0.995;
  return 1;
}

// ---- pure: finding the delay probe's tone ----------------------------------------------------
// The size of a 2 kHz tone in `win` (samples, -1..1), by Goertzel's method, and the share of the
// window's energy that is that tone. A window of exactly 10 ms holds whole cycles of it.
function liveTone(win, rate) {
  const n = win.length;
  if (!n) return { amp: 0, fraction: 0 };
  const c = 2 * Math.cos(2 * Math.PI * 2000 / rate);
  let s1 = 0, s2 = 0, e = 0;
  for (let i = 0; i < n; i++) {
    const x = win[i];
    const s = x + c * s1 - s2;
    s2 = s1; s1 = s; e += x * x;
  }
  const power = s1 * s1 + s2 * s2 - c * s1 * s2;
  const amp = 2 * Math.sqrt(Math.max(0, power)) / n;
  const fraction = e > 0 ? Math.min(1, (amp * amp / 2) / (e / n)) : 0;
  return { amp, fraction };
}

// ---- pure: which way to hear ----------------------------------------------------------------
// The way this device hears its speaker. `pref` is the owner's choice: "auto", "live" or
// "listening". Automatic is live when an AirPlay door is wired to the speaker (casting from a
// device: the picture is in front of you), and listening for radio, Spotify and the rest.
function liveModeFor(pref, sources, wires, speaker) {
  if (pref === "live" || pref === "listening") return pref;
  const door = (wires || []).some((w) => w.speaker === speaker
    && (sources || []).some((s) => s.id === w.source && s.kind === "airplay"));
  return door ? "live" : "listening";
}

// What actually plays: in the background only a media element carries on, so it is listening.
function liveEffectiveMode(mode, hidden) {
  return hidden ? "listening" : mode;
}

// ---- pure: the delay probe's arithmetic -----------------------------------------------------
// The hub put a tone in the mix; the page heard it. `m` has the page's clock (ms, from
// performance.now) when it asked (tSend), when the hub answered (tRecv), when it heard the tone
// (tDetect) and, for live, when the frame the tone began in arrived (tFlag, or null); and the
// hub's clock (us) when the request arrived, when the tone went into the mix, and when it
// answered. The two clocks are never compared: the time between the hub's own readings is
// taken off the round trip, half the rest is the way there, and the tone went into the mix a
// known time after the request arrived. Returns { ms, oneWay, transport, buffer }; transport and
// buffer split a live delay into the way to this page and the wait in its buffer.
function liveProbeDelay(m) {
  const hubWorked = (m.replied - m.received) / 1000;
  const oneWay = Math.max(0, ((m.tRecv - m.tSend) - hubWorked) / 2);
  const tMixed = m.tSend + oneWay + (m.injected - m.received) / 1000;
  const out = { ms: m.tDetect - tMixed, oneWay, transport: null, buffer: null };
  if (m.tFlag !== null && m.tFlag !== undefined) {
    out.transport = m.tFlag - tMixed;
    out.buffer = m.tDetect - m.tFlag;
  }
  return out;
}

// "live · 0.4 s": a delay for the card.
function liveDelayLabel(ms) {
  return `${(Math.max(0, ms) / 1000).toFixed(1)} s`;
}

// ---- the worklet ----------------------------------------------------------------------------
// The processor that plays the frames. It is built into a script out of the pure functions
// above (so there is one copy of each), and runs on the audio thread. Messages in: { t: "frame",
// seq, rate, l, r }, { t: "probe", on, ref }, { t: "level", v }, { t: "sync", extra, min } (the
// lip-sync plan: ms of delay added, and the buffer's new minimum). Messages out: { t: "stat",
// depthMs, target, playing, under, gaps, lost }, { t: "playing", on } and { t: "tone", at }
// (the context time the probe's tone began to come out).
//
// The buffer is a ring of three seconds. It begins to play when it holds the target and stops
// when it runs dry, to fill to the target again. Wherever the sound jumps (a gap in the numbers,
// a skip to catch up) and wherever it is about to run out, the last 5 ms before the jump fade
// out and the first 5 ms after fade in, so a gap is a short dip and never a click.
function liveWorkletBody() {
  class EndoraLive extends AudioWorkletProcessor {
    constructor(options) {
      super();
      const o = (options && options.processorOptions) || {};
      this.jit = o.jitter || { target: 300, min: 300, max: 800, calm: 0 };
      // Lip-sync: a fixed extra delay on top of the target, or a lower minimum.
      this.extra = 0;
      if (o.sync !== undefined) this.lipSync(liveSyncPlan(o.sync, this.jit.min));
      this.window = new Float32Array(Math.max(1, Math.round(sampleRate / 100)));
      this.tap = new Float32Array(128);
      this.setRate(o.rate || 44100);
      this.expected = null;
      this.under = 0; this.nGaps = 0; this.lost = 0;
      this.smooth = 0; this.sinceStat = 0;
      this.level = 1; this.probe = null; this.gain = 0;
      this.playing = false;
      this.port.onmessage = (e) => this.heard(e.data);
    }

    lipSync(plan) {
      this.extra = plan.extra;
      this.jit = liveRebase(this.jit, plan.min);
    }

    setRate(rate) {
      this.rate = rate;
      this.cap = Math.ceil(rate * 3);
      this.l = new Float32Array(this.cap); this.r = new Float32Array(this.cap);
      this.wr = 0; this.rd = 0;
      this.gaps = []; this.last = -1e12; this.skip = null;
      this.fade = Math.max(1, Math.round(rate * 0.005));
    }

    heard(m) {
      if (m.t === "level") { this.level = m.v; return; }
      if (m.t === "sync") { this.lipSync({ extra: m.extra, min: m.min }); return; }
      if (m.t === "probe") { this.probe = m.on ? { ref: m.ref, found: false } : null; return; }
      if (m.t !== "frame") return;
      if (m.rate && m.rate !== this.rate) { this.setRate(m.rate); this.expected = null; this.playing = false; }
      const step = liveSeqStep(this.expected, m.seq);
      if (step.stale) return;
      this.expected = (m.seq + 1) >>> 0;
      if (step.missing > 0) {
        this.lost += step.missing; this.nGaps += 1;
        this.jit = liveJitter(this.jit, { kind: "gap" });
        this.smooth = 0;
        this.gaps.push(this.wr);
      }
      const n = m.l.length;
      for (let i = 0; i < n; i++) {
        const at = (this.wr + i) % this.cap;
        this.l[at] = m.l[i]; this.r[at] = m.r[i];
      }
      this.wr += n;
      if (this.wr - this.rd > this.cap) { this.rd = this.wr - this.cap; this.gaps.push(this.rd); }
    }

    process(inputs, outputs) {
      const out = outputs[0], L = out[0], R = out[1] || out[0];
      const n = L.length;
      const dt = n / sampleRate * 1000;
      const per = this.rate / 1000;
      // What it holds, in ms, when all is well: the target and any lip-sync delay on top.
      const want = this.jit.target + this.extra;
      if (!this.playing && this.wr - this.rd >= want * per) {
        this.playing = true; this.last = this.rd; this.smooth = 0; this.gain = 0;
        this.port.postMessage({ t: "playing", on: true });
      }
      let i = 0;
      if (this.playing) {
        // Far ahead of its target (a burst, or a target that shrank): skip ahead to it, after
        // fading out over the next 5 ms, so the jump is a dip and never a click.
        if (!this.skip && this.wr - this.rd > (want + 150) * per) {
          this.skip = { at: this.rd + this.fade, to: this.wr - want * per };
        }
        const step = this.rate / sampleRate * liveServo((this.wr - this.rd) / per, want);
        for (; i < n; i++) {
          while (this.gaps.length && this.gaps[0] <= this.rd) this.last = this.gaps.shift();
          if (this.skip && this.rd >= this.skip.at) {
            this.rd = Math.max(this.rd, this.skip.to); this.last = this.rd; this.skip = null;
          }
          if (this.wr - this.rd < 2) {
            this.playing = false; this.under += 1; this.smooth = 0;
            this.jit = liveJitter(this.jit, { kind: "underrun" });
            this.port.postMessage({ t: "playing", on: false });
            break;
          }
          const i0 = Math.floor(this.rd), f = this.rd - i0;
          const a = i0 % this.cap, b = (i0 + 1) % this.cap;
          let g = Math.min(1, (this.rd - this.last) / this.fade, (this.wr - this.rd) / this.fade);
          if (this.gaps.length) g = Math.min(g, (this.gaps[0] - this.rd) / this.fade);
          if (this.skip) g = Math.min(g, (this.skip.at - this.rd) / this.fade);
          // The gain itself moves by at most 5 ms' worth a step, so sound that arrives just as
          // the buffer ran nearly empty brings the volume back up smoothly, never with a jump.
          const slew = 1 / this.fade;
          this.gain = this.gain + Math.max(-slew, Math.min(slew, g - this.gain));
          g = this.gain;
          const left = (this.l[a] * (1 - f) + this.l[b] * f) * g;
          const right = (this.r[a] * (1 - f) + this.r[b] * f) * g;
          this.tap[i] = left;
          L[i] = left * this.level; R[i] = right * this.level;
          this.rd += step;
        }
      }
      for (let k = i; k < n; k++) { L[k] = 0; R[k] = 0; this.tap[k] = 0; }
      if (this.playing) {
        this.smooth += dt;
        if (this.smooth >= 1000) { this.jit = liveJitter(this.jit, { kind: "tick", ms: this.smooth }); this.smooth = 0; }
      }
      if (this.probe && !this.probe.found) this.listen(n);
      this.sinceStat += dt;
      if (this.sinceStat >= 250) {
        this.sinceStat = 0;
        this.port.postMessage({
          t: "stat", depthMs: Math.max(0, (this.wr - this.rd) / per), target: this.jit.target,
          playing: this.playing, under: this.under, gaps: this.nGaps, lost: this.lost,
        });
      }
      return true;
    }

    // Looks for the probe's tone in the last 10 ms of what came out (before the level, so the
    // owner's volume does not hide it).
    listen(n) {
      const w = this.window, size = w.length;
      if (n >= size) w.set(this.tap.subarray(n - size, n));
      else { w.copyWithin(0, n); w.set(this.tap.subarray(0, n), size - n); }
      const t = liveTone(w, sampleRate);
      const p = Math.min(1, t.amp / this.probe.ref);
      if (p < 0.4 || t.fraction < 0.5) return;
      this.probe.found = true;
      // The window is p tone and the rest silence; the tone began p of a window before now.
      this.port.postMessage({ t: "tone", at: currentTime + n / sampleRate - p * size / sampleRate, amp: t.amp });
    }
  }
  registerProcessor("endora-live", EndoraLive);
}

// The worklet's script.
function liveWorkletSource() {
  return [liveSeqStep, liveJitter, liveRebase, liveSyncPlan, liveServo, liveTone].map((f) => f.toString()).join("\n")
    + "\n(" + liveWorkletBody.toString() + ")();\n";
}

// ---- in the browser: the live player --------------------------------------------------------
const LIVE = {
  ctx: null,        // one audio context for the page, made in a tap so it may make sound
  loading: null,    // the worklet being loaded into it
  session: null,    // the stream playing now: { node, gain, abort, ... }
};

function liveSupported() {
  return typeof window !== "undefined" && typeof AudioWorkletNode !== "undefined"
    && !!(window.AudioContext || window.webkitAudioContext)
    && typeof ReadableStream !== "undefined" && typeof fetch === "function";
}

// The page's audio context. Called from a tap the first time (Safari lets a page make sound
// only after one), which also lets it carry on after: later switches need no tap.
function liveContext() {
  if (LIVE.ctx) return LIVE.ctx;
  if (!liveSupported()) return null;
  const Context = window.AudioContext || window.webkitAudioContext;
  try { LIVE.ctx = new Context({ latencyHint: "interactive", sampleRate: LIVE_RATE }); }
  catch (_) { try { LIVE.ctx = new Context(); } catch (e) { return null; } }
  return LIVE.ctx;
}

function liveModule(ctx) {
  if (!LIVE.loading) {
    const url = URL.createObjectURL(new Blob([liveWorkletSource()], { type: "application/javascript" }));
    LIVE.loading = ctx.audioWorklet.addModule(url).catch((e) => { LIVE.loading = null; throw e; });
  }
  return LIVE.loading;
}

// Opens a live stream (the node's `url`, with its ticket) and starts playing it into the
// speakers, silently if `muted`. Returns the session: the stream goes on until `liveClose`.
// `on` has callbacks: stat(s), playing(bool), ended(), tone(at).
async function liveOpen(url, muted, on, sync) {
  const ctx = liveContext();
  if (!ctx) throw new Error("live mode isn't available in this browser");
  await liveModule(ctx);
  if (ctx.state !== "running") await ctx.resume();
  if (ctx.state !== "running") throw new Error("the browser would not start live sound without a tap");
  const node = new AudioWorkletNode(ctx, "endora-live", {
    numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2],
    processorOptions: { rate: LIVE_RATE, jitter: { target: LIVE_MIN, min: LIVE_MIN, max: LIVE_MAX, calm: 0 }, sync: Number(sync) || 0 },
  });
  const gain = ctx.createGain();
  gain.gain.value = muted ? 0 : 1;
  node.connect(gain); gain.connect(ctx.destination);
  const abort = new AbortController();
  const s = { node, gain, abort, on: on || {}, stat: null, playing: false, flagAt: null, lastAt: performance.now(), closed: false, toneWaiter: null };
  node.port.onmessage = (e) => {
    const m = e.data;
    if (m.t === "stat") { s.stat = m; if (s.on.stat) s.on.stat(m); }
    else if (m.t === "playing") { s.playing = m.on; if (s.on.playing) s.on.playing(m.on); }
    else if (m.t === "tone" && s.toneWaiter) { const w = s.toneWaiter; s.toneWaiter = null; w(m.at); }
  };
  (async () => {
    try {
      const res = await fetch(url, { signal: abort.signal, cache: "no-store" });
      if (!res.ok || !res.body) throw new Error("HTTP " + res.status);
      const reader = res.body.getReader();
      const parser = liveParser();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        const now = performance.now();
        s.lastAt = now;
        for (const f of parser.push(value)) {
          if (f.flags & 1) s.flagAt = now;
          node.port.postMessage({ t: "frame", seq: f.seq, rate: f.rate, l: f.left, r: f.right }, [f.left.buffer, f.right.buffer]);
        }
      }
    } catch (_) { /* closed on purpose, or the stream dropped: told below */ }
    if (!s.closed && s.on.ended) s.on.ended();
  })();
  return s;
}

// Lip-sync, applied at once to the stream playing (ms; positive plays later). Returns the plan,
// whose `short` says an earlier offset than the buffer can give was asked for.
function liveSetSync(s, ms) {
  const plan = liveSyncPlan(ms, LIVE_MIN);
  if (s && !s.closed) s.node.port.postMessage({ t: "sync", extra: plan.extra, min: plan.min });
  return plan;
}

function liveClose(s) {
  if (!s || s.closed) return;
  s.closed = true;
  try { s.abort.abort(); } catch (_) {}
  try { s.node.port.onmessage = null; s.node.disconnect(); s.gain.disconnect(); } catch (_) {}
}

// A tiny silent WAV. Played in a tap, it lets the page's <audio> element start the MP3 later
// by itself (when the page goes to the background), which Safari allows for an element that has
// played once from a tap.
function liveSilence() {
  const samples = 800, bytes = new Uint8Array(44 + samples * 2);
  const v = new DataView(bytes.buffer);
  const text = (at, s) => { for (let i = 0; i < s.length; i++) v.setUint8(at + i, s.charCodeAt(i)); };
  text(0, "RIFF"); v.setUint32(4, 36 + samples * 2, true); text(8, "WAVEfmt ");
  v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
  v.setUint32(24, 8000, true); v.setUint32(28, 16000, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
  text(36, "data"); v.setUint32(40, samples * 2, true);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return "data:audio/wav;base64," + btoa(bin);
}

// ---- measuring the delay --------------------------------------------------------------------
// The owner taps "Measure delay". The hub replaces the speaker's mix with a short tone, and the
// page notes when it comes out of each way of hearing: live (the worklet's output) and
// listening (a second <audio> element on the MP3, read by an analyser). Neither includes the
// device's own speaker or Bluetooth delay, which no page can see (`outputLatency` is added
// where the browser says it). Each is good to about 10 ms.

// The hub's answer to a probe, with the page's own clock readings; or an error.
async function liveProbe(speaker) {
  const tSend = performance.now();
  const reply = await api("POST", `/v1/sound/speakers/${speaker}/probe`, {});
  const tRecv = performance.now();
  return { tSend, tRecv, hub: reply };
}

// Live: the tone's arrival at the worklet's output, by the page's clock.
async function liveMeasureLive(speaker) {
  let s = LIVE.session, own = false;
  if (!s || s.closed) {
    const t = await api("POST", `/v1/sound/listen/${speaker}/ticket`, {});
    s = await liveOpen(t.live, true, {});
    own = true;
  }
  try {
    const begun = performance.now();
    while (!s.playing) {
      if (performance.now() - begun > 6000) throw new Error("live sound did not start");
      await new Promise((r) => setTimeout(r, 50));
    }
    const heard = new Promise((resolve) => { s.toneWaiter = resolve; });
    s.flagAt = null;
    s.node.port.postMessage({ t: "probe", on: true, ref: LIVE_TONE.amp });
    const p = await liveProbe(speaker);
    const at = await Promise.race([heard, new Promise((r) => setTimeout(() => r(null), 5000))]);
    if (at === null) throw new Error("the tone did not come back through live");
    const ctx = LIVE.ctx;
    const tDetect = performance.now() - (ctx.currentTime - at) * 1000 + ((ctx.outputLatency || 0) * 1000);
    const r = liveProbeDelay({ tSend: p.tSend, tRecv: p.tRecv, tDetect, tFlag: s.flagAt,
      received: p.hub.received_at_us, injected: p.hub.injected_at_us, replied: p.hub.replied_at_us });
    return { ...r, intake: p.hub.intake || [], outputMs: Math.round((ctx.outputLatency || 0) * 1000) };
  } finally {
    s.node.port.postMessage({ t: "probe", on: false });
    if (own) liveClose(s);
  }
}

// Listening: the tone's arrival at the end of the MP3, through a second element and an analyser.
async function liveMeasureListening(speaker) {
  const ctx = liveContext();
  if (!ctx) throw new Error("this browser cannot measure listening");
  if (ctx.state !== "running") await ctx.resume();
  const t = await api("POST", `/v1/sound/listen/${speaker}/ticket`, {});
  const el = new Audio();
  el.src = t.url;
  const src = ctx.createMediaElementSource(el);
  const analyser = ctx.createAnalyser();
  analyser.fftSize = 2048;
  const sink = ctx.createGain();
  sink.gain.value = 0;
  src.connect(analyser); analyser.connect(sink); sink.connect(ctx.destination);
  const size = Math.round(ctx.sampleRate / 100), buf = new Float32Array(analyser.fftSize);
  try {
    await el.play();
    // Steady state first: the browser has filled its buffer and is playing.
    const begun = performance.now();
    while (el.currentTime < 1.5) {
      if (performance.now() - begun > 30000) throw new Error("the MP3 did not start playing");
      await new Promise((r) => setTimeout(r, 100));
    }
    let found = null;
    const watch = setInterval(() => {
      if (found !== null) return;
      analyser.getFloatTimeDomainData(buf);
      const now = performance.now();
      const tone = liveTone(buf.subarray(buf.length - size), ctx.sampleRate);
      const p = Math.min(1, tone.amp / LIVE_TONE.amp);
      if (p >= 0.4 && tone.fraction >= 0.5) found = now - p * 10;
    }, 5);
    const p = await liveProbe(speaker);
    const waited = performance.now();
    while (found === null && performance.now() - waited < 25000) await new Promise((r) => setTimeout(r, 20));
    clearInterval(watch);
    if (found === null) throw new Error("the tone did not come back through listening");
    const tDetect = found + ((ctx.outputLatency || 0) * 1000);
    const r = liveProbeDelay({ tSend: p.tSend, tRecv: p.tRecv, tDetect, tFlag: null,
      received: p.hub.received_at_us, injected: p.hub.injected_at_us, replied: p.hub.replied_at_us });
    return { ...r, intake: p.hub.intake || [], outputMs: Math.round((ctx.outputLatency || 0) * 1000) };
  } finally {
    try { el.pause(); el.removeAttribute("src"); el.load(); src.disconnect(); } catch (_) {}
  }
}

// What the card says about the last measurement.
function soundMeasured(m) {
  if (!m) return "";
  const part = (name, r) => {
    if (!r) return "";
    if (r.error) return `${name}: ${r.error}`;
    const split = r.transport !== null && r.transport !== undefined
      ? ` (${Math.round(r.transport)} ms to this page, ${Math.round(r.buffer)} ms in its buffer)` : "";
    return `${name}: ${liveDelayLabel(r.ms)}${split}`;
  };
  const parts = [part("Live", m.live), part("Listening", m.listening)].filter(Boolean);
  const waits = ((m.live && m.live.intake) || (m.listening && m.listening.intake) || [])
    .filter((x) => x.ms > 0).map((x) => `${x.source} waits ${x.ms} ms in the hub`);
  return parts.join(" · ") + (waits.length ? ` · ${waits.join(", ")}` : "")
    + " · not counting this device's own speaker";
}

// "Measure delay": live first (a second or two), then listening (the browser fills its buffer
// before the tone can come out of it, so it takes several seconds).
async function soundMeasure() {
  const mine = soundMine();
  if (!mine || !SOUND.listening || SOUND.measuring) return;
  SOUND.measuring = true; SOUND.measure = null; render();
  const result = {};
  for (const [name, run] of [["live", liveMeasureLive], ["listening", liveMeasureListening]]) {
    try { result[name] = await run(mine.id); }
    catch (e) { result[name] = { error: e.message }; }
    SOUND.measure = result;
    try { console.info("endora sound: measured delay", name, JSON.stringify(result[name])); } catch (_) {}
  }
  SOUND.measuring = false; render();
}
