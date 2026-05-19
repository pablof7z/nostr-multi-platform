# ADR-0013 — NIP-29 Metadata-Signer Trust Model

> **Status:** Accepted (landed alongside T42 / commit `5178cfc`).
> **Date:** 2026-05-18.
> **Companion:** `docs/design/nip29/moderation.md` §4; `docs/plan/m11.5-highlighter.md` §Step 1 (last bullet).

## Context

NIP-29 group metadata events (kinds 39000–39003) are signed by **the relay's
own keypair**, not by any user. NMP must answer: which pubkey is *the* relay's
keypair, and how does the framework respond when that pubkey changes?

Three trust models were considered (`moderation.md` §4.1):

- **A. NIP-11-driven (strict).** Read the relay's NIP-11 document at HTTP
  fetch time; require `event.pubkey == nip11.pubkey` for every 39000-39003.
- **B. First-write-wins (TOFU).** Record the signer of the first **39000** we
  see for `(host_relay_url, group_id)`; reject subsequent 39000-39003 from a
  different signer until the user explicitly accepts a rotation.
- **C. Best-effort.** Accept any 39000-39003 received over the wire from
  `host_relay_url`. Trust the WebSocket/TLS layer to prevent forgery.

## Decision

**Ship A + B; explicitly reject C.**

The ingest hook for 39000-39003 enforces the following step ladder (per
`moderation.md` §4.3):

1. **NIP-11 strict (A)** when the host declares a `pubkey` in its NIP-11
   document: require `event.pubkey == nip11.pubkey`.
2. **TOFU steady state (B)** when `(group_id, signer)` is in the pinned
   cache: require `event.pubkey == cached_signer`.
3. **Cold TOFU bootstrap.** Only kind:39000 may establish the pin; 39001 /
   39002 / 39003 are held in a quarantine buffer (max 64 per group). When
   the first legitimate 39000 lands, the quarantine is replayed against
   the now-pinned signer; events from the right signer are applied, events
   from a wrong signer are rejected.
4. **Mismatch on (1) or (2):** reject the event (return
   `TrustCheckOutcome::Rejected` from `TofuSignerCache::evaluate`); the
   ingest pipeline drops it and leaves canonical state unchanged. A typed
   `MetadataSignerChanged` diagnostic surface + toast plumbing is deferred
   per "Open questions" below.

## Why C is rejected

Codex review of the M11.5 design surfaced a P1 spoofing vector:

> Any host relay that also accepts ordinary parameterized events would
> forward a user-signed kind:39001 carrying the room's `d` tag if it accepts
> the write. Since `GroupAdmins`/`GroupMembers` are derived *only* from these
> snapshots, accepting any signer-from-host-relay lets a malicious user spoof
> admin/membership state simply by signing and pushing a kind:39001. TLS
> authenticates the connection, not `event.pubkey`.

Policy B + the cold-TOFU rule (39000 only) defeats this:
- Pinning only from 39000 closes the cold-cache spoofing window — even a
  malicious user-signed 39000 is detectable because the legitimate
  relay-signed 39000 (which arrives via the same relay's normal metadata
  stream) will conflict on `event.pubkey` and trigger `MetadataSignerChanged`
  immediately on the next session.
- The quarantine buffer prevents user-signed 39001/39002 from establishing a
  pin under any circumstances.

## Consequences

**Positive:**

- Spoofing-via-host-relay attack is structurally blocked at ingest time.
- Hosts that declare NIP-11 pubkeys get the stronger guarantee (A) for free.
- Hosts that don't declare NIP-11 pubkeys still get B; the user opts into a
  trust decision implicitly on the first 39000 (TOFU is the industry-standard
  fallback for unverifiable identity bootstraps).
- The audit-only moderation policy (`moderation.md` §5) plus the
  cache-pinning trust model means `GroupAdmins` / `GroupMembers` are the
  **only** projection allowed to mutate from canonical relay-signed
  metadata — user-signed 9000/9001 actions can't corrupt them.

**Negative:**

- A relay that legitimately rotates its keypair will (once the typed
  diagnostic surface ships) trigger `MetadataSignerChanged` notifications
  for every joined group. T42 / Step 0 ships only the policy enforcement
  (rejecting events from a wrong signer); both the typed
  metadata-signer-error plumbing and the user-explicit-accept rotation UX
  are deferred to post-M11.5 per `moderation.md` §4.3.
- Quarantine adds a small RAM overhead (64 events × ~256 bytes × N
  groups). Bounded at 64 per group by `push_quarantine`; TTL-eviction
  ("after 1 hour" per `moderation.md` §4.3) is **deferred** — T42's
  in-memory implementation has the size bound but no `now_ms` expiry
  path. Tracked under "Open questions" below.

**Neutral:**

- The pinned signer cache **should** persist across sessions (M3 LMDB)
  so it doesn't re-prompt on restart. T42 ships the cache as
  in-memory only (`cache/tofu.rs::TofuSignerCache` holds a `BTreeMap`).
  LMDB-backed persistence is **deferred** to Step 5 alongside the
  rest of the M3 store wiring for `nmp-nip29` records.

## Implementation

Lands as `nmp_nip29::cache::TofuSignerCache` per M11.5 Step 0 (T42 /
commit `5178cfc`). The quarantine + replay logic is exercised by
`cache::tofu::tests::tofu_*` (`tofu_first_39000_pins_signer`,
`tofu_quarantines_39001_before_39000`, `nip11_strict_match_rejects_mismatch`)
in `crates/nmp-nip29/src/cache/tofu.rs`.

Bootstrap-host discovery (`routing.md` §4.3) already requires NIP-11 + 39000
signer match before caching a host candidate; this ADR governs the subsequent
ingest of already-pinned hosts.

## Open questions deferred

- **Typed `MetadataSignerChanged` diagnostic / toast plumbing.** T42 lands
  policy enforcement (`TofuSignerCache::evaluate` returns
  `TrustCheckOutcome::Rejected`); a typed error type that propagates to
  the diagnostics lane + a user-visible "metadata signer changed" toast
  is post-M11.5. Drop-on-rejection is the current behavior.
- **Quarantine TTL eviction.** `moderation.md` §4.3 specifies "TTL-evicted
  after 1 hour"; T42 enforces the size bound (64 entries per group via
  `push_quarantine`) but does not consume a `now_ms` for time-based
  expiry. Adding the TTL is a localized change in `cache::tofu` — defer
  until the actor wires a clock source through.
- **Persistence (M3 LMDB).** The pinned signer cache should survive
  session restarts so users aren't re-prompted on every relaunch. T42's
  `TofuSignerCache` is in-memory only; M3 LMDB hookup lands in M11.5
  Step 5 alongside the `nmp-nip29` record persistence.
- **Rotation UX** — the per-group "trust the new key?" prompt. Requires
  the typed-diagnostic surface above to land first; post-M11.5.
- **Per-relay vs per-group pin granularity** — currently per-group (per
  `(host_relay_url, local_id)`) for B; per-host for A (NIP-11 declares one
  pubkey for the relay). If a host rotates and we want to roll the
  quarantine + re-pin across all its groups atomically, the cache would need
  a per-host index too. Defer until a real-world rotation case forces it.

Related: ADR-0009 (kernel-boundary doctrine), ADR-0012 (RelayPinnedInterest,
landed alongside this ADR).
