# T126 — One-socket-per-URL invariant audit

**Verdict: PASS for byte-identical URL keys.** The invariant holds for every
production-reachable path that delivers `OutboundMessage.relay_url` as the
same `String` across roles. One real gap remains and is documented as **F7**:
relay-URL string normalization is absent system-wide, so if upstream ingestion
(kind:10002 verbatim clone path) produces two variant strings for the same
logical relay, the pool keys them as distinct sockets. Relays match on
host:port and will reject the duplicate — exactly the user-defined failure
mode. Not a regression vs. pre-T105; flagged as future hardening.

## One-line answers

- **Does `RelayControl.role` gate the spawn key?** NO. The spawn gate is
  `if relay_controls.contains_key(&relay_url)` at
  `crates/nmp-core/src/actor/relay_mgmt.rs:91-93`. `role` is stored only as a
  diagnostic-lane label (`#[allow(dead_code)]` at `actor/mod.rs:148-150`) and
  is **never** read for routing, lookup, or comparison after construction.
- **Can two roles for the same URL create two sockets?** NO for
  byte-identical URL strings — `ensure_relay_worker` short-circuits at
  `relay_mgmt.rs:91` on `contains_key` and the existing `RelayControl.tx` is
  reused. The only `relay_controls` mutations are `insert` (URL-keyed, gated)
  at `relay_mgmt.rs:97` and `drain()` at `relay_mgmt.rs:216` — no `remove`,
  no `entry().or_insert_with(...)`, no role-aware branching.
  YES if upstream ingestion produces URL-variant strings for the same logical
  relay (`wss://r.io` vs `wss://r.io/`); see F7 below.
- **Are URL strings normalized before keying?** NO. A repo-wide grep
  (`normalize`, `trim_end_matches`, `to_ascii_lowercase` over `crates/`)
  returns ZERO normalization helpers for relay URLs. NIP-65 tags flow
  through `parse_nip65_tags` (`publish/nip65/mod.rs:123-153`) which clones
  the tag value verbatim after only an `is_relay_url` scheme prefix check
  (`mod.rs:179-183`). User input through `add_relay` only `trim()`s
  (`actor/commands/relays.rs:22`). NWC URIs are pushed through as parsed.

## Per-scenario findings (task §2)

| Scenario | Verdict | Evidence |
|---|---|---|
| 1. URL in 2+ `RoutingSource` lanes (NIP65 + UserConfigured + Hint + Provenance) | PASS | Planner: `RelayPlan` is keyed by `RelayUrl` in a `BTreeMap` (`planner/plan.rs:174`); `role_tags: BTreeSet<RoutingSource>` aggregates lanes per URL. Partitioner accumulates into `entry.2.insert(RoutingSource::*)` at `partition/case_a_authors.rs:53,65,81,91` — same URL key, multiple roles. Pool: `ensure_relay_worker` short-circuits on byte-equal URL key (`relay_mgmt.rs:91`). |
| 2. URL string normalization mismatch (`/` suffix, case, scheme, port) | PASS (with caveat) | No normalization exists. Risk surface: kind:10002 tags are byte-cloned (`publish/nip65/mod.rs:131,137-148`). Two authors publishing `wss://r.io` vs `wss://r.io/` would seed two `BTreeMap<RelayUrl, RelayPlan>` entries → two `OutboundMessage.relay_url` strings → two pool entries. NOT a regression vs. pre-T105 (which had the same gap); flagged as future hardening F7 below. |
| 3. NIP-42 AUTH-paused relay reconnect | PASS | `partition_auth_paused` (`kernel/requests/mod.rs:325`) is a kernel-side queue partition, downstream of the pool. `Failed` / `Closed` handlers (`actor/dispatch.rs:291-301`) do NOT mutate `relay_controls`; the stale `Sender<RelayCommand>` stays — next `ensure_relay_worker` hits `contains_key == true` and returns without spawning. No re-dial path exists today (separately documented as F5/T125 reconnect gap). |
| 4. Bunker NIP-46 transport | PASS (transport not wired) | `sign_in_bunker` (`actor/commands/identity.rs:224-237`) shape-validates the URI and surfaces a "not wired" toast. No `nmp-core → nmp-signers` socket path exists; cannot create a second socket. |
| 5. NWC wallet relay (`2afa4b1`) | PASS | All three NWC outbound sites construct `OutboundMessage { role: RelayRole::Wallet, relay_url: nwc_uri.relay_url.clone(), .. }` (`actor/commands/wallet.rs:131,182,316`) flowing through the same `send_outbound` → `ensure_relay_worker` seam. The wallet relay URL collides with `Content` / `Indexer` only if a relay operator runs both functions on the same URL, in which case (per §1) the pool correctly serves both lanes from one socket. `wallet.is_nwc_relay(&relay_url)` is a string `==` check (`wallet.rs:57`) — read-only, no pool mutation. |

## `RelayControl.role` audit (task §3 — F5 status)

`RelayControl.role: RelayRole` at `actor/mod.rs:149`:

- `#[allow(dead_code)] // Diagnostic lane label; per-URL health is M11.`
- Set once at construction (`relay_mgmt.rs:101`).
- Never read at any call site (grep `control\.role|\.role\b` inside
  `crates/nmp-core/src/actor/` returns only comments and the
  `RelayEditRow.role` field, which is a separate struct for the UI edit
  projection).
- Not part of the `HashMap` key. Not a `PartialEq` / `Hash` participant.
- Not consumed by any diagnostic projection: `kernel/status.rs` builds its
  `RelayStatus` rows from `RelayRole::all()` (`status.rs:19`), not from the
  `RelayControl` table.

**F5 conclusion:** The `role` field on `RelayControl` is a load-bearing
NO-OP. It already behaves as the "diagnostic-only label that does not gate
the HashMap key" target described in F5 of
`docs/design/outbox-explorer-diagnostics.md` §7. No code change required for
the invariant; the field can be removed entirely when its FFI consumer
(`relay_diagnostics` per-URL projection, F3/F4) lands and supersedes the
diagnostic intent.

## Recommendation

1. Leave the pool implementation as-is. The invariant holds.
2. Add a regression test that locks in the multi-lane → single-socket
   property for byte-identical URLs across `Content`, `Indexer`, and
   `Wallet`.
3. Document the URL-normalization gap as follow-up **F7** — to be addressed
   if/when (a) kind:10002 ingestion observes two variant strings for the
   same logical relay in the wild, or (b) user-configured-relay sockets are
   wired (today `add_relay` is projection-only — see
   `actor/commands/relays.rs:1-9`). Normalization must land at every URL
   string-key boundary in one pass (`ensure_relay_worker`, `get`, worker
   echo-back, `is_nwc_relay`, `snapshot_active_wire_subs`) — half-fixing it
   would create silent miss-routes.

## Test added

`crates/nmp-core/src/actor/relay_mgmt.rs` — module `tests`:
- `same_url_two_roles_yields_one_control` — Content then Indexer with the
  same URL leaves `relay_controls.len() == 1`, retains the **first**
  inserted role (no rebinding), and the second `ensure_relay_worker` call
  returns `false` (not newly spawned).
- `same_url_three_roles_including_wallet_yields_one_control` — adds Wallet
  on top to lock in the post-`2afa4b1` NWC path.

## Cross-references

- Invariant doc: `docs/design/outbox-explorer-diagnostics.md` §1 (b2c295d).
- F5 (RelayControl.role aggregate): `docs/design/outbox-explorer-diagnostics.md` §7.
- T105 transport pool: `crates/nmp-core/src/actor/relay_mgmt.rs` module doc.
- ADR-0021 (relay-roles): `docs/decisions/0021-relay-roles-indexer-and-app-relay.md`.
