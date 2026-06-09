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
| A2 | No bespoke Nostr implementation routing around NMP (hand-rolled relay socket, REQ/EVENT framing, signer, MLS, NIP impl) | D0 | any | block | semantic + `WebSocket`/`URLSession.*wss`, `\["REQ"`, `secp256k1`, raw nip libs |
| A3 | No `switch`/`if` branching on event `kind` in shell | D0 | swift/kotlin/ts | block | `kind ==`, `\.kind\b`, `switch .*kind` |
| A4 | App composes generic interest/action APIs; no app domain noun pushed into kernel calls | D0 | any | warn | semantic |

## B — Best-effort rendering: placeholders, no gates (D1, C13)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| B1 | No `if !profile { spinner }` / `if missing { hide }` render gate | D1 / C13 | swift/kotlin/ts | block | `if .*== nil`/`?? ` guarding a view; `ProgressView` keyed on profile load |
| B2 | Display fields render via placeholders, not `Option`-gated UI | D1 / C13 | swift/kotlin/ts | warn | optional-chained display field gating render |
| B3 | Late authoritative data updates the payload in place (no flicker / re-fetch gate) | D1 / C13 | any | warn | semantic |

## C — Routing is automatic: no relays in app code (D3, C6, C7)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| C1 | No relay URL on any app-facing open/send/publish call (`relays:` param) | D3 | any | block | `relays:`, `relayUrls`, `wss://` literal in shell |
| C2 | No hardcoded relay constants in app code; routing resolves via outbox | D3 / C6 | any | block | `"wss://` literal, relay list constant |
| C3 | DM / gift-wrap never falls back to public relays — fail closed | D3 / C7 | any | block | semantic + public-relay fallback in DM path |

## D — Don't write the fallback (framework-magic contract)

| ID | Rule | Origin | Layer | Sev | Detection |
|----|------|--------|-------|-----|-----------|
| D1 | No app-side "is this newer?" / `replaceIfNewer` supersession | C1 | any | block | `replaceIfNewer`, `created_at >`, newest-wins logic in shell |
| D2 | No app-side kind:3 watch / `recompileSubsOnFollowChange` re-opening views | C5 | any | block | semantic + contact-list observer re-opening subs |
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
| F1 | No `try/catch` / `do { try } catch` around a framework call in shell | D6 | swift/kotlin/ts | block | `do {` / `try {` / `catch` wrapping an `nmp_`/kernel call |
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
| H1 | App consumes generated typed bindings; no hand-written `Decodable`/JSON parse over the snapshot | D5 / D6 | swift/kotlin/ts | block | hand-written `Decodable` of kernel payloads; `JSONDecoder().decode` on snapshot |
| H2 | No raw JSON string-keying of kernel snapshot/delta in app code | D5 | any | warn | `json["..."]`, dictionary access on snapshot |
| H3 | Writes go through typed action payloads, not raw event taps / bespoke publish doors | D0 / D6 | any | block | semantic + raw event-tap publish in shell |

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
| K1 | App doesn't compute replaceable-resolution / expiration / `created_at` trust; kernel owns time | D9 | any | warn | `created_at` comparison / TTL decision in shell |

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
