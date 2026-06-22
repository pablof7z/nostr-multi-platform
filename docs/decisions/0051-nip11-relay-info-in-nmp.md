# ADR-0051: First-class NIP-11 relay-information documents in NMP

Status: Accepted / Implemented

> Implemented in `crates/nmp-nip11` (`RelayInfoDoc`, the fetch/parse helpers),
> with `ActorCommand::SetRelayInfo` and the `RelayConnectedHook` seam live in
> `nmp-core`.

## Context

NIP-11 lets a client fetch a relay's *information document* — `name`,
`description`, `icon`, operator `pubkey`/`contact`, `software`/`version`, the
`supported_nips` array, and `limitation` block — by issuing an HTTP `GET` on
the `https://` form of the relay's `wss://` URL with the header
`Accept: application/nostr+json`. Apps use this everywhere a relay is shown:
the diagnostics screen, the "add relay" preview, the relay-picker list.

Today every consumer app hand-rolls this. Highlighter (`app/core`) carries its
own `probe_nip11` in `src/relay_polish.rs` and a `Nip11Document` model in
`src/models.rs` — it builds the `wss→https` URL, sets the `Accept` header, does
the HTTP round-trip, parses the JSON, and maps the fields. That is exactly the
"app hand-rolls HTTP + protocol parsing" pattern the framework thesis exists to
eliminate (`docs/aim.md` §2 commandment #4: *no native business logic*; a
relay-metadata fetch is a generic Nostr building block, not an app's
proprietary domain — `AGENTS.md` "What belongs in NMP crates").

The deferred design intent already exists in two places:

- `crates/nmp-network/src/pool/types.rs:174-176` defers a "NIP-11 capability
  map" to "phases C/D".
- `docs/research/relay-lifecycle-and-pools.md` §4.1(2) specifies a
  `RelayCapabilities` report per relay (`nip11_supported_nips`, …) framed D7:
  *the kernel reports; the planner/protocol crates decide*.

This ADR resolves that deferred intent and the Highlighter product correction
("NMP should own the full NIP-11 lifecycle so apps get relay metadata with zero
work").

## Decision

### 1. Placement — a new `nmp-nip11` protocol crate, not `nmp-core`

NIP-11 fetch + parse lives in a new Layer-4 protocol crate `nmp-nip11`,
structurally identical to `nmp-blossom` (ADR-0043) and the `nmp-nip57` LNURL
fetcher. `nmp-core` imports **no HTTP crate** and learns **no NIP-11 noun**
(D0). The crate owns:

- `RelayInfoDoc` — the parsed document (the canonical NMP shape).
- `http_url_for_relay(wss_url) -> Option<String>` — `wss://→https://`,
  `ws://→http://` mapping (pure, unit-tested).
- `parse_relay_info(url, body) -> RelayInfoDoc` — tolerant serde parse
  (every field optional; unknown fields ignored).
- `fetch_relay_info_blocking(wss_url) -> Result<RelayInfoDoc, String>` — the
  `ureq` GET with `Accept: application/nostr+json`, bounded by a timeout and a
  max-response-bytes cap (mirrors `nmp-nip57`/`nmp-blossom`). Runs on a spawned
  `std::thread`, never the actor thread (D8).

`RelayInfoDoc` is a substrate-generic transport-metadata struct — it carries no
domain *nouns* the way NIP-29 `group_id` does, so a *carried-through* copy may
live on the kernel's per-URL transport row (see §3). D0's banned-token set is
NIP-29-specific; "relay info" is generic transport telemetry, the same class as
the existing `RelayStatus` fields. The HTTP and parse *logic* stays out of
`nmp-core`.

### 2. Two trigger paths, one type, one cache

**(a) Automatic, on connect.** When a relay's socket opens
(`PoolEvent::Opened` in the actor), the actor invokes a new substrate-generic
`RelayConnectedHook` slot (mirrors the `RelayTextInterceptorSlot` injection
seam, V-38). `nmp-nip11` installs a hook that — subject to a per-URL TTL — spawns
the fetch worker. On success the worker posts a substrate-generic
`ActorCommand::SetRelayInfo { relay_url, doc_json }` back into the actor loop;
the dispatch arm stores it on the kernel's transport row and marks the snapshot
dirty. Failures are non-fatal: the relay simply has no document, and the TTL
gate prevents a hot retry loop.

**(b) On-demand probe.** An async FFI entry point
`nmp_app_probe_relay_info(url)` lets apps preview a relay that is **not** in the
pool (the "add relay" flow). It runs the same `fetch_relay_info_blocking` on a
worker and returns the same `RelayInfoDoc` JSON. The pool-relay path needs *no*
explicit per-relay probe call from the app — the data appears in diagnostics as
it resolves.

**Cache / TTL.** Freshness is governed by `nmp_core::util::TimeCached` (whose
doc-comment already cites NIP-11 as its motivating use case, 5-minute TTL). The
per-URL last-fetch instant is held kernel-side on the transport row; the hook
consults it before spawning a worker. Documents persist in RAM for the session
(the diagnostics row already lives there); on-disk persistence (the `nmp-nip29`
TOFU pattern) is **not** shipped here — NIP-11 is cheap to refetch and a stale
on-disk doc is worse than a fresh miss. A follow-up issue tracks persistence if
profiling shows churn.

### 3. Exposure — through the existing diagnostics surface

The parsed document flows through `RelayStatus` → `relay_diagnostics_snapshot()`
→ the `relay_diagnostics` projection apps already consume (JSON subtree **and**
the `KRDG` typed FlatBuffers sidecar, ADR-0037). `RelayDiagnosticsRow` gains an
optional `info` child carrying: `name`, `description`, `icon`, `pubkey`,
`contact`, `software`, `version`, `supported_nips`, and the `limitation`
booleans that are cheap to surface (`payment_required`, `auth_required`,
`restricted_writes`). Absent until the fetch resolves; `null`/`has_info=false`
otherwise. No app does HTTP, JSON, or knows what NIP-11 is — it reads
`row.info.name` and friends.

## Consequences

- Highlighter deletes `probe_nip11` and its `Nip11Document` model; its
  integration becomes a thin field-map from the diagnostics `info` child.
- `nmp-wasm` is unaffected: it does not depend on `nmp-nip11` (the `ureq`-using
  crate), exactly as it does not depend on `nmp-blossom`/`nmp-nip57`. The
  `RelayConnectedHook` slot is generic and HTTP-free, so `nmp-core` stays
  wasm-clean.
- The `nmp-network` "NIP-11 capability map deferred to phases C/D" comment is
  superseded: capability data is surfaced via diagnostics (D7 report), not via a
  pool-internal map. The pool stays substrate-grade.

## Subjective decisions / tradeoffs

- **Diagnostics-only, not routing.** This ADR surfaces NIP-11 as a *report*
  (D7). It deliberately does **not** wire `supported_nips` into planner routing
  (`docs/design/intent-routing/ffi-tests-future.md` §11 leaves NIP-11 routing
  as a future option).
  Consumers that want to filter relays by capability read the diagnostics
  `supported_nips`; the planner does not yet consume it.
- **No on-disk persistence.** Chosen for simplicity (Article VII) and because a
  stale relay doc is low-value; revisit per the follow-up issue if churn shows.
- **Hook slot vs. an app action.** The automatic fetch is not an app action
  because no app *requests* it — it is a lifecycle reaction. The injected-hook
  seam keeps `nmp-core` from naming `nmp-nip11`, preserving the dependency
  direction (D0).
