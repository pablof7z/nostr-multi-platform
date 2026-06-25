# NMP app-conformance catalog

> **Status: SEED.** Audience: app builders + the `nmp-conformance` scanner skill.
> This catalog is the **consumer-facing** re-cut of the doctrine (D0–D10) and the
> framework-magic contract (C1–C13, F-TTL). It enumerates the ways an app that
> *consumes* NMP can drift from correct usage.
>
> **Single source of truth.** This file adds only what is genuinely new — a
> consumer-side **detection signature** and a **severity** per rule. The *why*,
> the worked examples, and the canonical statement live in the cited **Origin**
> (the builder-guide / doctrine canon). Do not re-explain a doctrine here; cite
> it. Every `Origin` is validated against the live canon by the drift gate
> (`crates/nmp-testing/tests/conformance_catalog_complete.rs`, modeled on
> `contract_surface_complete`): a rule whose origin no longer maps to a live
> doctrine/contract bullet fails CI. Links don't stop drift; the gate does.
>
> **Scope.** Rules here apply to *app shell* code — Swift / Kotlin / TS and the
> C-ABI seam — **not** to `crates/` (that surface is owned by `doctrine-lint`).
> The two are complementary; neither duplicates the other.

Columns: **ID** · **Rule** (the thing the app must / must not do) ·
**Origin** (doctrine/contract bullet — the canon) · **Layer** (where it applies)
· **Sev** (block / warn / note) · **Detection** (consumer-side signature the
scanner looks for; `semantic` = needs LLM judgment, no reliable grep).

## A — Boundary: no business logic, no bespoke routing (D0)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| A1 | No Nostr business logic in shell code (event interpretation, reducers, NIP semantics) | D0 | swift/kotlin/ts | block | semantic + `kind ==`, `switch.*kind`, NIP-name fns in shell |
| A2 | No bespoke Nostr implementation routing around NMP (hand-rolled relay socket, REQ/EVENT framing, signer, MLS, NIP impl) | D0 | any | block | semantic + `\["REQ"`, `secp256k1`/`schnorr`/`bech32`, direct nostr-sdk/NDK import. NAME-TRAP: a `*RelayBridge` / `wss://` may be a non-Nostr socket (TTS/LLM stream) — judge the protocol, not the name |
| A3 | No `switch`/`if` branching on event `kind` in shell | D0 | swift/kotlin/ts | block | `kind` read on a kernel **event/snapshot** type (`SignedNostrEvent`, claimed-event DTO) — NOT a domain enum (`DownloadKind`/`AdKind`/`error.kind`); in a thick app ~all `\.kind` hits are domain enums |
| A4 | App composes generic interest/action APIs; no app domain noun pushed into kernel calls | D0 | any | warn | semantic |
| A5 | No shell-side reduction of raw kernel events (kind-dispatch, NIP-10 thread building, tag parsing); consume a typed projection | D0 / D5 | any | block | semantic — a raw `SignedNostrEvent`/event-with-`tags` crossing into the shell is itself the smell; the fix is a kernel-side typed projection (D5), not shell code |

## B — Best-effort rendering: placeholders, no gates (D1, C13)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| B1 | No `if !profile { spinner }` / `if missing { hide }` render gate | D1 / C13 | swift/kotlin/ts | block | `if .*== nil`/`?? ` guarding a view; `ProgressView` keyed on profile load |
| B2 | Display fields render via placeholders, not `Option`-gated UI | D1 / C13 | swift/kotlin/ts | warn | optional-chained display field gating render |
| B3 | Late authoritative data updates the payload in place (no flicker / re-fetch gate) | D1 / C13 | any | warn | semantic |

## C — Routing is automatic: no relays in app code (D3, C6, C7)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| C1 | No relay-routing override (`relays:` param) on an app-facing open/send/publish call | D3 | any | block | `relays:`/`relayUrls:` as a *routing arg* — EXCLUDE NIP-65/NIP-17 relay-list *payloads* (the list is the event content, not a route) |
| C2 | App MAY declare a default/bootstrap relay set; MUST NOT pass per-call routing relays | D3 / C6 | any | block | per-call `relays:` override — EXCLUDE app default-seed into `configured_relays`/`addRelay` bootstrap (required; you cannot outbox-resolve from zero relays) |
| C3 | DM / gift-wrap never falls back to public relays — fail closed | D3 / C7 | any | block | semantic + public-relay fallback in DM path |

## D — Don't write the fallback (framework-magic contract)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| D1 | No app-side "is this newer?" / `replaceIfNewer` supersession | C1 | any | block | `replaceIfNewer`, `created_at >`, newest-wins logic in shell |
| D2 | No app-side dynamic-source watch re-opening views | C5/#2092 | any | block | semantic + contact/list/pointer observer re-opening subs |
| D3 | No `dedupeAcrossRelays` / app-side cross-relay event merge | C9 | any | block | `dedup`, `Set<EventId>` merge across relays |
| D4 | No manual REQ historical-backfill scan; no `supports_nip77 = true` assumption | D2 | any | warn | `"REQ"` for backfill; `supportsNip77 = true` |
| D5 | No profile-refresh poll / manual TTL timer / stale-check before render | F-TTL | any | block | `refreshProfileEvery`, `Timer.*profile`, TTL math in shell |

## E — Single writer per fact; no parallel state (D4)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| E1 | No app-side cache parallel to `AppState` (SwiftData/Room store mirroring kernel state) | D4 | swift/kotlin | block | `@Model`/Room entity mirroring kernel facts |
| E2 | Account switch is a state transition, not tear-down/rebuild of view handles | D4 / C12 | any | warn | semantic + view-handle teardown on account change |
| E3 | Exactly one writer per fact; downstream derives mechanically | D4 | any | warn | semantic |

## F — No errors across FFI (D6)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| F1 | No `try/catch` / `do { try } catch` around a framework call in shell | D6 | swift/kotlin/ts | block | `catch` whose `try` body calls `nmp_`/kernel — EXCLUDE stdlib `JSONEncoder`/`JSONDecoder`/FlatBuffer decode of app-side *input* (those legitimately throw) |
| F2 | Every failure surfaces via observable state (toast / `busy` clears / diagnostic) | D6 | any | warn | semantic |
| F3 | No new per-operation error enum plumbed through FFI | D6 | any | warn | error enum crossing the C-ABI |

## G — Capabilities report, never decide (D7)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| G1 | No native code deciding retry / recoverability / relay / cipher / resulting state | D7 | swift/kotlin | block | `if shouldRetry`/`if recoverable` in native capability |
| G2 | Capability start/stop/restart is idempotent; no state beyond OS handles | D7 | swift/kotlin | warn | semantic + cached state past handle lifetime |

## H — Typed interfaces, not hand-rolled JSON (codegen / FlatBuffers)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| H1 | App consumes generated typed bindings; no hand-written `Decodable`/JSON parse over the snapshot | D5 / D6 | swift/kotlin/ts | block→warn¹ | hand-written `Decodable` of kernel payloads; `JSONDecoder().decode` on snapshot |
| H2 | No raw JSON string-keying of kernel snapshot/delta in app code | D5 | any | warn | `json["..."]`, dictionary access on snapshot |
| H3 | Writes go through typed action payloads, not raw event taps / bespoke publish doors | D0 / D6 | any | block | semantic + raw event-tap publish in shell |
| H4 | Shell uses an NMP-provided npub/bech32 display helper; no hand-rolled NIP-19 encoding (even for display) | D0 / D5 | any | note | hand-rolled `Bech32.encode` / NIP-19 TLV in shell — currently a **framework gap**: NMP exposes no npub helper, so this is a `note` until it does |

## I — Compose self-claiming components (no app-side fetch)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| I1 | Use NMP components (NostrAvatar / NostrProfileName / embed cards) that self-claim; don't resolve kind:0 in app code | D1 / F-TTL | swift/kotlin/ts | warn | app code calling `claimProfile`/kind:0 fetch outside a component |
| I2 | No proactive kernel fetch from the app on event ingest; components self-claim on render | D1 | any | warn | semantic |

## J — Reactivity & no polling (D8)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| J1 | No polling / sleep+check loop anywhere in shell; use blocking recv / OS callback / snapshot subscription | D8 | any | block | `Timer.scheduledTimer`, `sleep(`, `while.*poll`, `setInterval` |
| J2 | No per-event work on a hot path in shell; consume bounded screen-shaped snapshots | D8 / D5 | any | warn | semantic |

## K — Kernel owns time (D9)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| K1 | App doesn't compute replaceable-resolution / expiration / `created_at` trust; kernel owns time | D9 | any | warn | `created_at` in a *comparison/arithmetic* (`>`/`<`/`-`/TTL math) — EXCLUDE display formatting of a kernel-provided value |

## L — Provenance & privacy (D10)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| L1 | App doesn't forward a received event to another relay without explicit user intent | D10 | any | block | semantic + re-publish of received event |
| L2 | No logging of private / gift-wrap payloads or secret material | D10 | any | block | `print`/`log` of decrypted content, nsec, private payload |

## Notes for the scanner

- **`semantic` rules are the high-value ones.** Bespoke reimplementation (A2),
  composition-vs-reinvention (A4, I1), and parallel-state (E1) are why this
  scanner exists; grep signatures are only a *first pass* that the LLM tier
  confirms or discards.
- **Thin-shell apps pass A1/A3 by construction.** That is expected — the rules
  are a forcing function for *future, thicker* apps, not a critique of Chirp.
- **Severity is advisory to the report, not a CI gate.** `block` = "this
  reintroduces a bug class the framework already extinguished"; `warn` =
  "smells like drift, confirm intent"; `note` = informational.
- **¹ H1 drops to `warn` behind a schema-version reject gate.** Hand-decoding the
  snapshot is acceptable *only* if a `schema_version` mismatch is rejected
  (fails closed) rather than silently misparsed — that gate extinguishes the
  drift bug class H1 guards, leaving only DX/maintenance debt. Chirp's
  `KERNEL_SCHEMA_VERSION` check is the worked example. Without such a gate, H1
  is a `block`. (Origin: D5.) **The gate must be a live comparison that
  *rejects*** — a declared-but-never-compared `SCHEMA_VERSION` constant is
  cosmetic and earns no discount (Podcastr declares the constant but decodes
  leniently via `decodeIfPresent ?? default` and never rejects a mismatch, so a
  newer schema silently misparses — that is still a `block`).

## Calibration log (what real scans taught the catalog)

Each entry is a precision/recall lesson from a scan against a shipping app.
Keep the *signatures* honest — a rule that cries wolf trains builders to ignore
the scanner.

- **2026-06-09 · Chirp (iOS) first run.** Confirmed real drift: bespoke kind
  dispatch in `EmbedHost.swift` (A1/A2, root cause = kernel doesn't emit the
  typed `EmbedKindProjection` into the snapshot, so the shell re-runs
  `nmp-content::resolve_event`); a stray `kind == 6` in `ThreadNoteRow.swift`
  ignoring the already-projected `isRepost` (A3); the hand-decode `KernelBridge`
  surface (H1, schema-gated → `warn`). **Signature hardening applied** to
  C1/C2/F1/K1: each over-fired on a *legitimate* pattern — app default-relay
  seeding (C2), NIP-65/17 relay-list payloads (C1), stdlib `JSONEncoder` throws
  (F1), `created_at` display formatting (K1). The lesson: a `relays:` /
  `created_at` / `do-catch` token alone is not the harm; the harm is the
  *semantic role* (routing override vs. data; trust-arithmetic vs. display;
  wrapping an `nmp_` call vs. stdlib). The mechanical pass narrows; only the
  semantic pass convicts.
- **2026-06-09 · Podcastr (iOS) — first portable run against a separate repo,
  and the first *thick* app.** Scanned from this catalog as the source of truth
  against `~/Work/podcast-player` (a podcast player consuming NMP via symlink).
  Integration surface largely conformant (profile resolution, NIP-46, account
  creation, publish-signing all delegated; no bespoke relay socket or Swift
  signer). Real drift localized to the **feedback subsystem**: shell-side
  reduction of raw `SignedNostrEvent`s (A1/A3/**A5**) + app-side replaceable
  supersession (D1), plus clip/note NIP-tag building in Swift (A4) and a
  Blossom hand-built auth event (A2, documented waiver). **Three catalog
  changes added by this run:** (1) new rule **A5** — the feedback drift is the
  *same shape* as Chirp's `EmbedHost` (kernel emits raw events → shell reduces
  them); naming it points the fix at the kernel projection, not the shell.
  (2) new rule **H4** — shell hand-rolls NIP-19 bech32 for display because NMP
  exposes no npub helper (a framework gap; `note` until the helper ships).
  (3) footnote ¹ tightened — a declared-but-uncompared schema constant is a
  cosmetic gate, no `warn` discount. **Signature lesson (thick app):** A3's
  `\.kind\b` is near-useless here — 40+ `kind ==` hits were *all* domain enums
  (`DownloadKind`/`AdKind`/`error.kind`), zero Nostr; and name-traps abound
  (`AgentRelayBridge` = an LLM loop, `ElevenLabs wss://` = TTS audio). The
  catalog held the line *only* because the semantic pass discarded every one of
  these — a thick app is where "grep narrows, semantic convicts" stops being
  advice and becomes load-bearing.
