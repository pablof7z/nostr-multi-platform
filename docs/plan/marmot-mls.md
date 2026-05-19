# Post-v1 Marmot — MLS-over-Nostr Encrypted Groups

> **Status:** Deferred post-v1. M11.5 explicitly excludes encrypted groups; this milestone is the resolution path.
> **Implementation crate:** wraps [`marmot-protocol/mdk`](https://github.com/marmot-protocol/mdk) (Marmot Development Kit, Rust) — see §Architecture.
> **Companion docs:** [`post-v1.md`](post-v1.md) (summary entry), [`m11.5-highlighter.md`](m11.5-highlighter.md) §What this milestone does NOT ship (deferred pointer).

---

## What this is

The Marmot Protocol layers [MLS (RFC 9420)](https://www.rfc-editor.org/rfc/rfc9420.html) on top of Nostr relays to provide end-to-end encrypted group messaging with **forward secrecy** and **post-compromise security** — properties NIP-17 DMs and NIP-29 groups do not offer. Group state (the MLS ratchet tree, epoch secrets, key schedules) is managed cryptographically; relay operators cannot read message content and cannot learn group membership without additional metadata.

**Relationship to deferred NIP-17 (post-v1 M9):** These coexist. NIP-17 is the interop-standard DM protocol understood by the broader Nostr ecosystem; Marmot is a higher-security primitive for applications that require forward secrecy. A client shipping both offers NIP-17 DMs for general use and Marmot groups for high-trust contexts. NIP-59 gift-wrap is shared infrastructure — see §Prerequisites.

**Relationship to M11.5 NIP-29 groups:** Also coexist. NIP-29 provides relay-moderated public groups (relay sees all content, manages membership). Marmot provides cryptographically-sealed groups (relay is a dumb message bus; it cannot read content or determine group membership). They address different threat models.

---

## Architecture

### `nmp-marmot` crate

`nmp-marmot` wraps `mdk-core` (Marmot Development Kit, v0.7.1+) and adapts it to NMP's module surface:

- **`DomainModule` impls:** `MarmotGroup`, `MarmotMessage`, `MarmotKeyPackage`, `MarmotWelcome`
- **`ViewModule` impls:** `GroupList`, `GroupMessages`, `MemberList`
- **`ActionModule` impls:** `CreateGroup`, `InviteMember`, `SendMessage`, `LeaveGroup`, `RemoveMember`, `UpdateKeys`, `PublishKeyPackage`

The crate does **not** re-implement MLS. OpenMLS (the RFC 9420 implementation MDK builds on) provides all cryptographic operations; `nmp-marmot` is an adapter layer: MDK types → NMP module contracts.

### MLS ratchet state storage

MDK exposes a `mdk-storage-traits::StorageProvider` abstraction for MLS-specific state (group ratchet trees, epoch secrets, key packages, credentials). This state is fundamentally different from Nostr event records — it is mutable, versioned cryptographic state with its own consistency requirements.

**Decision:** `nmp-marmot` ships `mdk-sqlite-storage` for MLS ratchet state, in a dedicated SQLite file alongside NMP's LMDB event store. Rationale:
- MLS ratchet state is not Nostr event data — storing it in LMDB's event trees would violate the separation between protocol state and transport data.
- `mdk-sqlite-storage` is the upstream-tested backend for this specific use case. Writing a custom LMDB storage provider for OpenMLS's internal storage interface would be complex, fragile, and not load-bearing for any doctrine proof.
- This is not a doctrine violation: D4 ("single writer per fact; caches derive") applies to Nostr event facts, not to MLS epoch state. The kernel boundary (D0) is maintained because `nmp-core` holds no MLS types; the SQLite state lives entirely within `nmp-marmot`.
- The SQLite file is an implementation detail of the `nmp-marmot` crate; the rest of the system is unaware of it.

### Key packages and Nostr identity

MLS `KeyPackage` objects are published as Nostr events to relays, so other group members can fetch them for invitations. `nmp-marmot` uses the M6 signer surface to derive the MLS credential from the user's Nostr key, ensuring a single identity across Nostr and MLS.

### Group invitations and NIP-59

Marmot `Welcome` messages (the MLS group-join invitation) are delivered via Nostr gift-wrap (NIP-59) — the same envelope mechanism as NIP-17 DMs. `nmp-marmot` depends on a standalone `nmp-nip59` crate (see §Step 0) rather than importing the full NIP-17 DM surface.

### Relay routing

Marmot groups are relay-pinned: all group events route to a specific group relay. `nmp-marmot` uses the `RelayPinned` routing lane introduced in M11.5 (`InterestShape::relay_pin`, ADR-0012). No additional compiler changes are required — this is exactly the generic capability ADR-0012 promised subsequent relay-pinned NIPs would use.

---

## Scope

### Step 0 — `nmp-nip59` crate (gift-wrap, shared with NIP-17)

NIP-59 gift-wrap is a prerequisite for Marmot invitations. If post-v1 M9 (NIP-17 DMs) has already shipped when this milestone runs, `nmp-nip59` already exists and this step is a no-op. If not:

- Implement `nmp-nip59`: `GiftWrap` + `Seal` + `Rumor` event construction, parsing, encryption using NIP-44 (a prerequisite).
- `WelcomeWrap` ActionModule: takes an MLS `Welcome` blob, wraps it in a NIP-59 gift-wrap addressed to the recipient.
- `WelcomeUnwrap` DomainModule: unwraps incoming gift-wraps, routes MLS Welcome messages to MDK for processing.

If M9 shipped first, this step is replaced with importing `nmp-nip59` from the M9 crate set.

### Step 1 — `nmp-marmot` crate (core protocol surface)

- `MarmotGroup` DomainModule: tracks the MLS group state (members, epoch, ratchet tree snapshot for display purposes — the actual cryptographic state lives in MDK/SQLite).
- `MarmotMessage` DomainModule: decrypted message records, keyed by group + epoch + sender.
- `MarmotKeyPackage` DomainModule: tracks own and peers' published key packages (as Nostr events).
- `MarmotWelcome` DomainModule: tracks pending inbound Welcome messages.
- `GroupList` ViewModule: list of joined + pending Marmot groups with unread count.
- `GroupMessages` ViewModule: paginated decrypted message stream for a group; live-updates on new epoch.
- `MemberList` ViewModule: current group member list with MLS leaf indices.

### Step 2 — ActionModules (write path)

- `PublishKeyPackage`: generates a fresh MLS `KeyPackage`, signs with the M6 signer, publishes as a Nostr event to the user's configured relays.
- `CreateGroup`: creates an MLS group, publishes the first `GroupInfo` event, stores ratchet state in SQLite.
- `InviteMember`: fetches target's `KeyPackage` from relay, creates MLS `Welcome`, sends via NIP-59 gift-wrap.
- `SendMessage`: encrypts plaintext as an MLS `ApplicationMessage`, publishes to the group relay.
- `LeaveGroup`: sends an MLS `Remove` proposal (self-removal), publishes, cleans local state.
- `RemoveMember`: admin-only `Remove` proposal for another member.
- `UpdateKeys`: sends an MLS `Update` proposal + `Commit` to advance the epoch and rotate forward-secrecy material. Called automatically after `RemoveMember`; can be called manually by any member.

### Step 3 — Key package rotation

- Automatic key package rotation: after a `KeyPackage` is consumed by an incoming `Welcome`, publish a fresh one immediately.
- Stale key package expiry: monitor own published key packages; re-publish if the most recent is older than a configurable TTL (default: 7 days).
- On app launch: check relay for whether own key packages are present; publish if absent.

### Step 4 — Integration with relay routing

- All `nmp-marmot` interests declare `InterestShape::relay_pin(group_relay_url)` — uses the ADR-0012 lane from M11.5.
- Key package events route via standard outbox (author-write, NIP-65 mailboxes).
- Welcome messages (gift-wrap) route to the recipient's NIP-65 inbox relays.

---

## Prerequisites

| Dependency | Why |
|---|---|
| M3 — LMDB persistence | `MarmotMessage` + `MarmotGroup` + `MarmotKeyPackage` records persist in LMDB (the records, not the MLS ratchet state). |
| M5 — NIP-42 auth | Group relays typically require NIP-42; `nmp-marmot` interests declare auth-required per the routing doc. |
| M6 — signers + write path | MLS credentials bind to Nostr keys; `PublishKeyPackage` + all ActionModules require a working signer. |
| M11.5 — relay-pin routing lane | `InterestShape::relay_pin` (ADR-0012) must exist; without it there is no clean way to pin all group events to a single relay. |
| NIP-44 encryption | Used by NIP-59 gift-wrap (Welcome delivery). Either ships with M9 or is implemented in Step 0. |
| NIP-59 gift-wrap | Welcome delivery. Ships in Step 0 if M9 has not yet run. |

---

## Exit gate (kernel boundary)

- `nmp-core` gains zero MLS types: no `MlsGroup`, `KeyPackage`, `Welcome`, `Epoch`, `RatchetTree`, `MarmotGroup`, or `MarmotMessage` types. Verified by grep at the PR.
- `nmp-marmot` is the sole importer of `mdk-core` and `openmls`. No other NMP crate depends on MLS types.
- The M2 compiler requires no changes: `InterestShape::relay_pin` already exists from M11.5; `nmp-marmot` uses it as any other relay-pinned crate does.
- The M2 publish planner requires no changes: Marmot events use standard author-write routing (key packages) or relay-pin (group messages); no new routing rules are needed.

## Exit gate (product)

- **Key package lifecycle end-to-end:** publish key package, fetch peer's key package from relay, create group, send Welcome, peer joins group.
- **Message round-trip:** send a message, peer receives and decrypts it.
- **Forward secrecy proof:** remove a member, send `UpdateKeys`, verify the removed member's epoch secrets cannot decrypt subsequent messages (MDK's `process_message` returns an error on the old credential).
- **Post-compromise security proof:** simulate compromise of a member's private key at epoch N; after that member calls `UpdateKeys` (epoch N+1), verify an attacker holding epoch-N secrets cannot derive epoch-N+1 secrets.
- **Key package rotation:** after a Welcome consumes the published key package, a fresh one is published automatically (verified by relay inspection).
- **Stale key package expiry:** after TTL, a fresh key package is published and the old one is superseded.

## Exit gate (perf)

- Group of 10 members, 100 messages: `GroupMessages` view renders ≤ 200 ms cold.
- `SendMessage` round-trip (encrypt → publish → relay-ack): ≤ 500 ms on Wi-Fi.
- `InviteMember` round-trip (fetch KeyPackage → create Welcome → deliver → peer join): ≤ 2 s on Wi-Fi.

---

## What this milestone does NOT ship

- **Marmot 1:1 DMs** — the MLS protocol supports 2-member groups, but using Marmot for 1:1 messaging overlaps with NIP-17; defer until NIP-17 interop picture is clearer.
- **Multi-device sync** — MLS supports multiple devices per identity (via linked key packages), but the UX and key-linking protocol is post-Marmot.
- **Group migration / forking** — MLS supports subgroup derivation; defer.
- **Encrypted media (MIP-04)** — MDK ships an optional `mip04` feature; this milestone uses plaintext message payloads only.
- **Marmot-native app UI** — this milestone delivers the `nmp-marmot` crate and a headless integration test. A consumer app (e.g., a Marmot-first messaging client) is a separate effort.

---

## Runnable artifact

- `crates/nmp-marmot/` — the protocol crate.
- `crates/nmp-nip59/` — the gift-wrap crate (if M9 has not yet run).
- `crates/nmp-testing/tests/marmot_*.rs` — integration tests covering all exit-gate scenarios.
- Report in `docs/perf/marmot/`:
  - `kernel-boundary.md` — grep evidence + code review sign-off.
  - `forward-secrecy-proof.md` — test output demonstrating the removed-member scenario.
  - `post-compromise-proof.md` — test output demonstrating the epoch-rotation scenario.
  - `perf-measurements.md` — round-trip latency numbers.
