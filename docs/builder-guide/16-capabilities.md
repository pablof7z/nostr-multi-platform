# 16 — Capabilities (D7)

> Status: **SHIPS**. Audience: both. Doctrine: **D7** (capabilities report; native never decides policy), **D6** (no error types across FFI), **D5** (snapshots/state bounded).

A capability is the **only** sanctioned way native code touches the world the
kernel cannot reach: the Keychain, the push token, the OS file picker, an
`AVPlayer`, a CoreML model. The contract is deliberately narrow so an
LLM-driven or novice app author *cannot* express the broken version — there is
no surface on which to put a retry loop, a relay choice, or a cached policy
decision in native code.

## The capability shape

`crates/nmp-core/src/substrate/capability.rs:1-24` is the entire substrate.
Three types, no more:

```rust
// crates/nmp-core/src/substrate/capability.rs:3-24
pub trait CapabilityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;                 // e.g. "core.audio_playback"
    type Request:  Clone + Serialize + DeserializeOwned + Send + 'static;
    type Result:   Clone + Serialize + DeserializeOwned + Send + 'static;
    fn callback_interface_name() -> &'static str;  // the native bridge name
}

pub struct CapabilityRequest  { namespace, correlation_id, payload_json }
pub struct CapabilityEnvelope { namespace, correlation_id, result_json }
```

Read this against the v1 catalog in
`docs/product-spec/api-surface.md:192-229` (§6.5): `KeyringCapability`,
`PushCapability`, `ExternalSignerCapability`, `NetworkMonitorCapability`,
`BlobPickerCapability` — each a callback interface native *implements* and Rust
*calls*. Note what is **absent**: no `Result<T, E>`, no return-by-throw, no
relay argument, no retry count. Native returns raw data via the reverse
callback; the kernel decides what it means. That absence is D7 and D6 enforced
by construction, exactly as `framework-magic/capabilities.md` describes the
rendering-side analog.

A protocol/app module defines its own capability the same way. A future media
app could define `AudioCapabilityRequest` (`Load`/`Play`/`Pause`/`Seek`/
`SetRate`/`Stop`) and an `AudioCapabilityEvent` bus whose
`Tick { current_s, duration_s }` is documented as "≤4 Hz while playing (D8:
coalesced, not per-frame)" — the bridge emits sparsely; the kernel owns the
cadence. Keep that crate app-owned until the capability proves reusable across
more than one app.

NIP-77 transport support is the same pattern at a different layer:
`crates/nmp-nip77/src/capability.rs:30-66` is a per-relay `CapabilityCache` /
`CapabilityProbe` — native (the relay) *reports* `NEG-MSG` / `NEG-ERR`; the
probe state machine (`Unknown → Probing → Supported|Unsupported`) is Rust
policy. The relay never decides "you should fall back to REQ"; the kernel's
coverage gate does (see [13 — Sync engine — `nmp-nip77`](13-sync-engine.md)).

## Decides vs reports — 8 worked examples

The single question for every capability boundary: **is this a fact about the
device, or a policy about what to do?** Facts cross the bridge. Policy never
does. Mix of core + podcast capabilities:

| # | Capability | Native (the bridge) **reports** | Kernel **decides** |
|--:|---|---|---|
| 1 | `KeyringCapability` | "stored", "here are the bytes for `account_id`", "not found" | which account is active, whether to re-encrypt, NIP-49 envelope shape, what to do on a miss |
| 2 | `PushCapability` | "registered", "here is the APNs token", "registration failed" | whether to register at all, when to re-register, which relays the token is announced to |
| 3 | `ExternalSignerCapability` | "user approved, signature = …", "user cancelled", "request timed out" | which event to sign, retry-after-cancel policy, account-mismatch rejection (ADR-0015) |
| 4 | `NetworkMonitorCapability` | "wifi", "cellular", "offline" — the raw link state | whether to pause sync, drain the publish queue, or hold REQs (D7: native does **not** decide to reconnect) |
| 5 | `BlobPickerCapability` | "user picked file at handle/URI, mime = …", "user dismissed" | upload target, Blossom server selection, chunking, retry on upload failure |
| 6 | `AudioPlayback` (future media app) | `StateChanged(Playing/Paused/Error)`, `Tick { current_s }`, `Ended` | which media item to load, resume position, rate, and skip policy; bridge obeys `Seek` |
| 7 | `EmbeddingCapability` (future app) | "vector = `[f32; 384]`", "model unavailable", "inference error" | which model, how to chunk overflow text, and retry policy |
| 8 | `TranscriptionCapability` (future app) | "language available", "model downloading 40%", "chunks = …", "cancelled" | when to transcribe, which language, what to do with the transcript, cancel-vs-wait policy |

If you find yourself wanting the bridge to "just retry the keychain read a
couple of times" or "fall back to a public relay if the picker fails", stop:
that is kernel policy. The bridge reports the failure as a result variant; the
kernel's action ledger decides. This is why a capability `Result` is an enum of
*facts* (`Loaded`, `StateChanged`, `Error { reason }`), never `Result<T,E>`.

## Idempotence checklist

Every capability is **idempotent** and **bounded** (`api-surface.md:229`).
Before a capability bridge is accepted, verify:

- [ ] **`start` after `start` is a no-op.** Re-registering push, re-arming a
      network monitor, re-loading the *same* audio URL is safe N times. The
      bridge does not refuse, does not double-fire; coalescing duplicate work
      is the *kernel's* job, not the bridge's.
- [ ] **`stop`/`cancel` after `stop`/`cancel` is a no-op.** No straggler
      events arrive after `Cancelled` (the M10.5 stress harness asserts this).
- [ ] **Restart-safe N times.** 1000-cycle start/stop/restart leaves zero
      retained-by-cycle leaks (the `ffi-stress` instrument, §K).
- [ ] **Bounded native state.** The bridge holds only OS handles
      (`AVPlayer?`, `timeObserver?`, weak sink). **No queue, no history, no
      preferences, no derived state, no policy.** "Skip the ad" is decided in
      Rust from `Tick` events. This is the D5 side of the contract.
- [ ] **No app/protocol noun in `Request` or `Result`.** If any field names
      an episode, a highlight, a group — redesign. The request types name
      URLs, paths, tokens, languages, prompts; nothing domain-specific
      This is what keeps capabilities reusable across future apps and keeps app
      nouns out of `nmp-core` (D0).
- [ ] **Failures are result variants, not exceptions.** `Error { reason:
      String }` crosses the bridge; a thrown native exception or a
      `Result<T,E>` does not (D6).

## Anti-patterns

- **Native retry policy.** A bridge that retries the keychain read or the
  upload three times before reporting failure. Retry cadence is kernel policy
  (D7); the bridge reports the *first* failure as a fact and lets the action
  ledger decide.
- **Capability holding cached state beyond OS handles.** A bridge that keeps a
  playback queue, a profile cache, or a settings dictionary. State that
  outlives the OS handle belongs in the EventStore or AppState (D4/D5), not the
  bridge. UserDefaults-as-backing is allowed *only* because Rust remains the
  source of truth (`capabilities.md` §F).
- **`Result`-typed errors instead of envelopes.** Exposing
  `fn sign(...) -> Result<Sig, SignError>` across the callback interface.
  Errors must arrive as a result *variant* in the JSON envelope (D6); a typed
  error across FFI is the exact bug the envelope shape rules out.
- **Native deciding which relay to publish to / when to reconnect.** The
  `NetworkMonitorCapability` reports link state; it must not call back "now
  reconnect to relay X". Routing and reconnect are kernel/planner concerns
  ([10 — Outbox routing](10-outbox-routing.md),
  [14 — Subscription lifecycle + relay manager + NIP-42](14-relay-manager.md)).
- **Non-idempotent `start`.** A bridge whose second `start` double-registers,
  double-fires, or errors. It must be a clean no-op; the stress harness will
  catch a leak otherwise.
- **App-noun fields.** `AudioRequest::PlayEpisode { episode_id }` instead of
  `Load { url_or_path }`. The noun leaks the app into the reusable substrate.

See also: [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md) ·
[05 — Kernel substrate — the 5 trait families](05-substrate-traits.md) ·
[11 — Sessions + signers + identity scopes](11-sessions-signers.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[17 — iOS shell — SwiftUI consumes the kernel](17-ios-shell.md)
