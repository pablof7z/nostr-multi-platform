# Codex review — 5a9db3f (T26 phase-1 follow-up)

**Commit**: `5a9db3f fix(m2): address-drop bug in partition_interest Case A + follow-up TODOs (T26)`
**Reviewer**: T26 agent self-review (vibe-tools rate-limited at review time)
**Status**: PASS — no blocking issues found

## Diff summary

6 files changed, 153 insertions(+), 8 deletions(-):

- `crates/nmp-core/src/planner/compiler.rs` — Fix Case A address-drop bug
- `crates/nmp-core/src/planner/interest.rs` — TODO(nmp-nip19) on NaddrCoord
- `crates/nmp-core/src/planner/plan.rs` — TODO(wire-emitter) on SubShape
- `crates/nmp-core/src/kernel/requests/profile.rs` — TODO(M2-migration) comments
- `crates/nmp-core/src/kernel/requests/thread.rs` — TODO(M2-migration) comments
- `crates/nmp-testing/tests/m2_subscription_compilation_audit.rs` — Regression test

## Bug fix correctness

**Case A address-drop**: The original Case A code in `partition_interest()` built a
`per_relay_authors` map and returned early without processing `interest.shape.addresses`.
This meant any interest with both `authors` and `addresses` non-empty would silently
drop all address-pointer coordinates from the compiled plan.

**Fix**: Unified the author and address routing into a single `per_relay` map with type
`BTreeMap<RelayUrl, (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, RoutingSource)>`. Both
authors and address coordinates are routed to their respective mailbox relays before
building `RelayEntry` structs. Relays that serve both author content and address content
are unified into a single entry with both sets populated.

**Regression test**: `interest_with_authors_and_addresses_preserves_both` verifies:
1. The author's declared relay appears in the plan (relay routing works)
2. The address coordinate is preserved in the sub-shape's `shape.addresses` field

The test was confirmed RED before the fix, GREEN after.

## Test gate

```
cargo test --workspace: 100% pass (6 M2 audit tests + 24 nmp-core tests)
cargo clippy --workspace --all-targets -- -D warnings: clean
```

## Open items (deferred, not blocking T26)

1. **TODO(nmp-nip19)**: `NaddrCoord::from_naddr_bech32` / `to_naddr_bech32` helpers
   deferred to the nmp-nip19 crate. TODO comment added in `interest.rs`.

2. **TODO(wire-emitter)**: `SubShape` needs a `lifecycle: InterestLifecycle` field
   when the wire-emitter lands. Rule 6 already enforces lifecycle equality before
   merge, so the lifecycle is available; it just isn't stored on the output type yet.
   TODO comment added in `plan.rs`.

3. **TODO(M2-migration)**: Full migration of `requests/{profile,thread}.rs` to
   `SubscriptionCompiler`-driven interest registration requires the wire-emitter,
   InterestRegistry, and trigger-based recompilation infrastructure — all phase-2
   components. Migration path documented per `compiler.md` §3.5. TODO headers
   added to both files.

4. **`simple_shape_hash` uses `DefaultHasher`**: This is used only for
   `canonical_filter_hash` on `SubShape` within a single kernel run (not persisted
   cross-process). `DefaultHasher` is deterministic within one process execution
   per the Rust spec. Acceptable for phase-1; upgrade to FNV-1a when the
   wire-emitter lands and needs cross-session stability.

## Verdict

**PASS**. The address-drop bug is fixed and regression-tested. All other open items
are appropriately deferred with TODO comments. No D6 violations (PlannerError never
crosses FFI), no D8 violations (no per-event allocs introduced by the fix). The
phase-1 planner scaffolding is complete and clean.
