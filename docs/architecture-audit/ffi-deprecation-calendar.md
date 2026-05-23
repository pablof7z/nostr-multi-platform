# Bespoke FFI Deprecation Calendar — D11 expansion

> Detail page for [PD-039](../BACKLOG.md#pd-039--bespoke-ffi-deprecation-calendar-d11-expansion--decision-made-2026-05-23).
> Companion to v1 exit criterion #7 in [`docs/plan.md`](../plan.md#v1-exit--what-has-to-be-true-to-ship).
> Verified against HEAD `4fd656dd` (2026-05-23).

## Why this exists

The bespoke `#[no_mangle] pub extern "C" fn nmp_app_*` surface in
`crates/nmp-core/src/ffi/` competes with the single canonical
`dispatch_action` seam. v1 exit criterion #7 says no new bespoke symbol may be
added once a deprecation calendar exists. This page IS that calendar: it sorts
every existing symbol into a category, fixes the migration cadence for the
debt subset, and names the doctrine reviewers apply to new additions.

## Enforcement

Already shipped:

- [`ci/check-ffi-surface-freeze.sh`](../../ci/check-ffi-surface-freeze.sh) +
  [`.github/workflows/ffi-surface-freeze.yml`](../../.github/workflows/ffi-surface-freeze.yml)
  reject net-additions in a PR diff. Genuinely-structural additions are
  exempted by adding the symbol name + ADR number to the `ADR_OVERRIDES`
  array. The single existing override is `nmp_app_is_alive` / ADR-0028 (the
  D7 actor-liveness probe), which is the precedent for future structural
  additions.
- [`ci/check-ffi-header-drift.sh`](../../ci/check-ffi-header-drift.sh) catches
  the inverse — `nmp_app_*` symbols declared in `ios/Chirp/Chirp/Bridge/NmpCore.h`
  with no Rust counterpart.
- D11 doctrine-lint
  ([`crates/nmp-testing/bin/doctrine-lint/rules/d11.rs`](../../crates/nmp-testing/bin/doctrine-lint/rules/d11.rs))
  blocks any new `nmp_app_*` body that constructs
  `ActorCommand::PublishSignedEvent` / `PublishUnsignedEvent` directly. The
  whitelist (`nmp_app_retry_publish`, `nmp_app_cancel_publish`) is the
  publish control plane that has no `dispatch_action` analogue.

## Doctrine — the Theme A discriminator

Codified at [`crates/nmp-core/src/substrate/action.rs`](../../crates/nmp-core/src/substrate/action.rs)
lines 4–47. A reviewer applies this rule to any new `nmp_app_*` proposal:

> *Is this a user or app intent to author or mutate Nostr-content state, where
> the kernel decides which identity signs and where it lands?* If yes,
> register an `ActionModule` and route through `dispatch_action`. If no — it
> is system-authored (kernel lifecycle, capability socket, observer
> registration, connection-oriented protocol glue, publish handle control
> plane, ack/liveness probe) — it MAY live on a bespoke entrypoint, but it
> MUST NOT construct `ActorCommand::PublishSignedEvent` /
> `PublishUnsignedEvent` inside its body (D11 lint catches that).

## Inventory totals on 2026-05-23 (HEAD `4fd656dd`)

Verified by `grep -rn 'pub extern "C" fn nmp_app_' crates/nmp-core/src/`:
**48** unique `nmp_app_*` symbols.

| Category | Count | Status |
|---|---:|---|
| **Canonical** — `nmp_app_dispatch_action` | 1 | keep forever |
| **Already a thin shim over `dispatch_action`** | 1 | counts toward done |
| **Test-only** (`cfg(any(test, feature = "test-support"))`, never in production ABI) | 4 | out of scope — D0-gated |
| **Structural permanent** (Theme A bespoke) | 26 | keep, freeze-locked |
| **Migration debt** (user-intent verbs that send `ActorCommand` directly) | 16 | migrate to `dispatch_action` |
| **Total** | **48** | |

The `nmp_app_*` surface OUTSIDE `nmp-core`
(`crates/nmp-signer-broker/src/`: 2 symbols; `apps/chirp/nmp-app-chirp/src/`:
8 symbols) is inside the freeze script's scope but outside this calendar's
scope: those are app-owned and named, follow the same "no new bespoke
without ADR" rule, and have no dispatch_action analogue (NIP-46 broker glue
and `nmp-app-chirp`'s C-ABI surface for the iOS shell).

## Migration calendar — cadence and batches

**Rule (in force from 2026-05-23):** No new `nmp_app_*` symbol may be added
without a merged ADR. The freeze-script gate rejects net-additions by default;
genuinely-structural additions are exempted via `ADR_OVERRIDES`. **Target: zero
migration-debt symbols at v1-B (post-v1 product milestone).** Structural
permanent (26) and canonical/shim (2) stay. Test-only (4) stay gated.

**Batches:**

- **Batch 1 — v1-A (pre-ship) deletions: 0 symbols.** Every debt symbol has at
  least one Swift call site on HEAD (verified by
  `grep -rln nmp_app_* ios/Chirp/`). No orphan debt symbols to delete pre-v1.
  The `NmpCore.h` drift (declares `nmp_app_react`, `nmp_app_follow`,
  `nmp_app_publish_signed_event`, …, all deleted from Rust between PR-F and
  ADR-0027) is a separate hygiene item — fix via the header-drift gate
  regeneration, not this calendar.
- **Batch 2 — v1-A → v1-B: 7 symbols, ~2/quarter.** Identity + relay-edit verbs
  first — they are the loudest pattern (every body sends
  `app.send_cmd(ActorCommand::*)` directly) and each has an obvious
  `ActionModule` namespace:
    - Q1: `nmp_app_signin_nsec` → `nmp.identity.signin_nsec`;
      `nmp_app_signin_bunker` → `nmp.identity.signin_bunker`.
    - Q2: `nmp_app_create_new_account` → `nmp.identity.create_account`;
      `nmp_app_switch_active` → `nmp.identity.switch_active`.
    - Q3: `nmp_app_remove_account` → `nmp.identity.remove_account`;
      `nmp_app_add_relay` → `nmp.relays.add`.
    - Q4: `nmp_app_remove_relay` → `nmp.relays.remove`; collapse the
      `creating_account_inflight` bespoke guard into the generic
      `inflight_dispatches` map (dead-code elimination follow-up).
- **Batch 3 — v1-B (post-v1 product): 9 symbols.** View/subscription registry
  mutations (the `open_*` / `close_*` cluster plus `claim_profile` /
  `release_profile`). Debatable under Theme A (they operate on the
  subscription registry, not on Nostr content) — keeping them bespoke is
  defensible. Migrating them is the cleaner long-term story (the kernel ends
  with exactly one user-facing C symbol that mutates state). Reclassification
  calls (`nmp_app_claim_profile`, `nmp_app_release_profile` — reference-count
  handles, not user actions; may stay bespoke) deferred to v1-B planning.

**Definition of done per migration:** the body becomes a thin
`dispatch_action_json(Some(app), "<namespace>", &serde_json::to_string(&action)?)`
shim — exactly the pattern `nmp_app_wallet_pay_invoice` already follows
([`crates/nmp-core/src/ffi/wallet.rs`](../../crates/nmp-core/src/ffi/wallet.rs)
lines 118–176). The C-ABI symbol is RETAINED as a thin wrapper for byte-stable
Swift compatibility; only the body changes. Net-zero ABI churn, full
`action_stages` lifecycle coverage on every migrated verb.

## Per-symbol inventory

### Canonical (1)

- `nmp_app_dispatch_action` — `crates/nmp-core/src/ffi/action.rs:99`.

### Already a thin shim (1)

- `nmp_app_wallet_pay_invoice` — routes through
  `dispatch_action("nmp.wallet.pay_invoice", …)` (V3 closure, PR #361);
  `crates/nmp-core/src/ffi/wallet.rs:119`.

### Structural permanent (26)

Grouped by file in `crates/nmp-core/src/ffi/`:

- `action.rs`: `nmp_app_ack_action_stage` (action lifecycle ack, `:140`);
  `nmp_app_register_action_result_observer` (push-side observer, `:196`).
- `capability.rs`: `nmp_app_set_capability_callback` (`:30`);
  `nmp_app_dispatch_capability` (`:56`); `nmp_app_free_string` (`:73`).
- `event_observer.rs`: `nmp_app_register_event_observer` (`:45`);
  `nmp_app_unregister_event_observer` (`:68`).
- `lifecycle.rs`: `nmp_app_lifecycle_foreground` (`:44`);
  `nmp_app_lifecycle_background` (`:64`); `nmp_app_set_lifecycle_callback`
  (`:81`); `nmp_app_is_alive` (D7 liveness probe, ADR-0028, `:125`).
- `mod.rs`: `nmp_app_new` (`:480`); `nmp_app_free` (`:1377`);
  `nmp_app_set_update_callback` (`:1387`); `nmp_app_set_storage_path`
  (`:1427`); `nmp_app_start` (`:1443`); `nmp_app_configure` (`:1460`);
  `nmp_app_stop` (`:1477`); `nmp_app_reset` (`:1485`).
- `publish.rs`: `nmp_app_retry_publish` (D11 whitelist, `:48`);
  `nmp_app_cancel_publish` (D11 whitelist, `:64`).
- `raw_event_tap.rs`: `nmp_app_register_raw_event_observer` (`:89`);
  `nmp_app_unregister_raw_event_observer` (`:115`).
- `snapshot.rs`: `nmp_app_register_snapshot_projection` (`:65`).
- `wallet.rs`: `nmp_app_wallet_connect` (NWC connection lifecycle, `:48`);
  `nmp_app_wallet_disconnect` (NWC connection lifecycle, `:64`).

### Test-only (4)

`cfg(any(test, feature = "test-support"))`, never in production ABI:

- `nmp_app_inject_pre_verified_events` — `ffi/testing.rs:35`.
- `nmp_app_inject_signed_events` — `ffi/testing.rs:105`.
- `nmp_app_inject_signed_event_json` — `ffi/testing.rs:162`.
- `nmp_app_read_projection_json` — `ffi/testing.rs:228`.

### Migration debt (16)

| Symbol | Batch | Planned namespace |
|---|---|---|
| `nmp_app_signin_nsec` (`ffi/identity.rs:21`) | 2 Q1 | `nmp.identity.signin_nsec` |
| `nmp_app_signin_bunker` (`ffi/identity.rs:37`) | 2 Q1 | `nmp.identity.signin_bunker` |
| `nmp_app_create_new_account` (`ffi/identity.rs:48`) | 2 Q2 | `nmp.identity.create_account` |
| `nmp_app_switch_active` (`ffi/identity.rs:104`) | 2 Q2 | `nmp.identity.switch_active` |
| `nmp_app_remove_account` (`ffi/identity.rs:115`) | 2 Q3 | `nmp.identity.remove_account` |
| `nmp_app_add_relay` (`ffi/identity.rs:137`) | 2 Q3 | `nmp.relays.add` |
| `nmp_app_remove_relay` (`ffi/identity.rs:153`) | 2 Q4 | `nmp.relays.remove` |
| `nmp_app_open_timeline` (`ffi/identity.rs:164`) | 3 (v1-B) | `nmp.timeline.open` |
| `nmp_app_open_author` (`ffi/timeline.rs:16`) | 3 (v1-B) | `nmp.timeline.open_author` |
| `nmp_app_open_thread` (`ffi/timeline.rs:31`) | 3 (v1-B) | `nmp.timeline.open_thread` |
| `nmp_app_open_firehose_tag` (`ffi/timeline.rs:46`) | 3 (v1-B) | `nmp.timeline.open_firehose_tag` |
| `nmp_app_open_uri` (`ffi/timeline.rs:62`) | 3 (v1-B) | `nmp.timeline.open_uri` |
| `nmp_app_claim_profile` (`ffi/timeline.rs:76`) | 3 (v1-B) | reclassification candidate (handle refcount, may stay bespoke) |
| `nmp_app_release_profile` (`ffi/timeline.rs:101`) | 3 (v1-B) | reclassification candidate (handle refcount, may stay bespoke) |
| `nmp_app_close_author` (`ffi/timeline.rs:126`) | 3 (v1-B) | `nmp.timeline.close_author` |
| `nmp_app_close_thread` (`ffi/timeline.rs:141`) | 3 (v1-B) | `nmp.timeline.close_thread` |

## Adjacent hygiene items (NOT in this calendar's scope)

- **`ios/Chirp/Chirp/Bridge/NmpCore.h` drift.** The hand-edited header still
  declares `nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow`,
  `nmp_app_publish_signed_event`, `nmp_app_publish_signed_event_to`,
  `nmp_app_publish_unsigned_event`, `nmp_app_register_action_module`,
  `nmp_app_register_action_executor` — Rust symbols deleted between PR-F and
  ADR-0027. `ci/check-ffi-header-drift.sh` is the enforcement seam; the
  cleanup is a v1-A hygiene item separate from this calendar.
- **`crates/nmp-signer-broker/src/ffi.rs`** declares
  `nmp_app_cancel_bunker_handshake` and `nmp_app_nostrconnect_uri`. Both are
  Theme A "system-authored" (NIP-46 broker glue), in the freeze-script scope
  but out of this calendar's scope.
- **`apps/chirp/nmp-app-chirp/src/ffi/`** declares 8 `nmp_app_chirp_*` symbols
  (app-owned, app-named — out of `nmp-core`).
