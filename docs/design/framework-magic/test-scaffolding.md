# Framework Magic — Test Scaffolding

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/design/subscription-compilation/tests.md` §9.3 (the `PlannerHarness` this scaffolding extends); `docs/design/lmdb/tests.md` (storage-layer test patterns); `docs/product-spec/subsystems.md` §7.13 (`nmp-testing` surface).

## 1. File location and naming convention break

```
crates/nmp-testing/tests/framework_magic_contract.rs
```

The existing convention is **milestone-prefixed** (`m2_subscription_compilation_audit.rs`, `m3_lmdb_invariants.rs`, etc.). This file is intentionally **cross-cutting** — it is the *only* test file in `crates/nmp-testing/tests/` that is not milestone-prefixed. The convention break is deliberate:

- The contract spans M2 + M3 + M4 + M6 + M8 + reactivity-bench; no single milestone owns it.
- Renaming the file under a single milestone (e.g., `m_cross_framework_magic.rs`) would suggest one milestone is responsible for the whole contract; the opposite is true — every milestone owner adds to it.
- The file is the *index test* — the meta-test (§4 below) reads `docs/design/framework-magic.md`'s row table and asserts every row has a `#[test] fn` with the expected name. A renaming under a milestone prefix would obscure this role.

The next milestone owner must **not** rename the file to fit the milestone-prefix pattern. The framework-magic delta protocol in `intro.md` §4 covers this: a milestone landing that renames `framework_magic_contract.rs` requires an ADR.

`Cargo.toml` for `nmp-testing` adds the standard `[[test]]` block:

```toml
[[test]]
name = "framework_magic_contract"
path = "tests/framework_magic_contract.rs"
```

Invocation: `cargo test -p nmp-testing --test framework_magic_contract`.

## 2. Test names — the canonical 14

Thirteen behavior tests (C1–C13; the table in `framework-magic.md` shows the exact names) plus the coverage meta-test, total 14 `#[test] fn` declarations:

```
c1_replaceable_supersedes_on_insert
c2_parameterized_replaceable_supersedes_by_dtag
c3_kind5_delete_removes_referenced_and_tombstones
c4_nip40_expiration_removes_and_persists_schedule
c5_kind3_change_recompiles_follow_dependent_subs
c6_authors_subscription_routes_to_per_author_write_relays
c7_publish_routes_outbox_and_private_fails_closed
c8_subscriptions_coalesce_autoclose_and_buffer
c9_provenance_merges_across_relay_redeliveries
c10_watermark_gates_backfill_and_authoritative_miss
c11_bunker_url_and_nsec_creation_complete_via_actions
c12_account_switch_rebinds_views_without_imperative_dance
c13_view_payload_uses_placeholders_then_refines_in_place
contract_surface_complete                                  # meta-test
```

Test names are **stable identifiers**. Renaming any of them constitutes a contract revision per `intro.md` §4 and requires the deprecation marker (`#[test] fn old_name() { c_n_new_name() }` for at least one milestone cycle).

## 3. The harness

The harness is the union of three existing testing surfaces, exposed as one builder:

```rust
// crates/nmp-testing/src/framework_magic.rs (proposed)

pub struct ContractHarness {
    actor:            TestActor,                  // wraps the real actor with a recorded reconciler
    planner:          PlannerHarness,             // from subscription-compilation/tests.md §9.3
    clock:            SimulatedClock,             // from subsystems.md §7.13
    network_chaos:    NetworkChaos,               // from subsystems.md §7.13
    mock_relays:      Vec<MockRelay>,             // from nostr-relay-builder
    keyring:          InMemoryKeyringCapability,  // for C11
    audit:            WireFrameAuditLog,          // proposed; captures every CLOSE/REQ/EVENT frame
    reconciler_log:   Vec<AppUpdate>,             // every AppUpdate emitted across the FFI seam
}

impl ContractHarness {
    pub fn new() -> Self;
    pub fn with_mock_relays(self, count: u8) -> Self;
    pub fn with_nip77_capable_relays(self, capable: &[bool]) -> Self;
    pub fn with_seeded_accounts(self, accounts: &[(Pubkey, SignerKind)]) -> Self;
    pub fn with_active_account(self, pubkey: Pubkey) -> Self;
    pub fn with_seeded_mailboxes(self, entries: &[(Pubkey, MailboxList)]) -> Self;
    pub fn with_seeded_follows(self, account: Pubkey, follows: &[Pubkey]) -> Self;
    pub fn build(self) -> Contract;
}

pub struct Contract {
    // dispatch surface
    pub fn dispatch(&mut self, action: AppAction);
    pub fn open_view<V: ViewModule>(&mut self, spec: V::Spec) -> ViewHandle<V>;
    pub fn close_view<V: ViewModule>(&mut self, handle: ViewHandle<V>);
    pub fn ingest(&mut self, relay: usize, event: NostrEvent);
    pub fn ingest_eose(&mut self, relay: usize, sub_id: &str);
    pub fn disconnect_relay(&mut self, relay: usize);
    pub fn reconnect_relay(&mut self, relay: usize);
    pub fn advance_clock_ms(&mut self, ms: u64);
    pub fn simulate_actor_restart(&mut self);

    // assertion surface
    pub fn wire_frames(&self, relay: usize) -> &[WireFrame];
    pub fn reconciler_log(&self) -> &[AppUpdate];
    pub fn event_store_get(&self, id: &EventId) -> Option<&StoredEvent>;
    pub fn provenance_of(&self, id: &EventId) -> &Provenance;
    pub fn watermark_of(&self, filter_sig: &FilterSig, relay: usize) -> Option<&Watermark>;
    pub fn action_ledger(&self) -> &[ActionLedgerRow];
    pub fn keyring_entries(&self) -> &[KeyringEntry];
    pub fn session_state(&self) -> &SessionState;
}
```

The harness extends `PlannerHarness` rather than wrapping it: every assertion the M2 audit gate makes against `PlannerHarness::compile_audit_log()` is accessible through the contract harness via `Contract::wire_frames(relay)`, but the contract harness also drives the full actor (so action ledger transitions, projection cache updates, and reconciler emissions are observable).

`InMemoryKeyringCapability` is a new `nmp-testing` primitive for C11. It implements the `KeyringCapability` trait with a `HashMap<String, Vec<u8>>` backing store; the test inspects the stored bytes to verify NIP-49 encryption envelope shape.

`WireFrameAuditLog` is a new `nmp-testing` primitive that captures every outbound frame the relay-worker emits. The M2 design has an audit log on the planner side; this harness has it on the wire side — both must agree, and a separate harness invariant could later assert that agreement.

The harness does **not** include a real network — every relay is a `MockRelay`. Every contract test runs in deterministic time with no I/O. Total runtime budget for the full suite: <5 seconds.

## 4. The coverage meta-test

```rust
#[test]
fn contract_surface_complete() {
    // 1. Read docs/design/framework-magic.md and parse the contract table.
    let contract = parse_contract_table(include_str!("../../../docs/design/framework-magic.md"));

    // 2. Enumerate the #[test] fns in this binary via inventory or a const list.
    //    The const list is the canonical surface; inventory is the consistency check.
    const EXPECTED_TESTS: &[&str] = &[
        "c1_replaceable_supersedes_on_insert",
        "c2_parameterized_replaceable_supersedes_by_dtag",
        "c3_kind5_delete_removes_referenced_and_tombstones",
        "c4_nip40_expiration_removes_and_persists_schedule",
        "c5_kind3_change_recompiles_follow_dependent_subs",
        "c6_authors_subscription_routes_to_per_author_write_relays",
        "c7_publish_routes_outbox_and_private_fails_closed",
        "c8_subscriptions_coalesce_autoclose_and_buffer",
        "c9_provenance_merges_across_relay_redeliveries",
        "c10_watermark_gates_backfill_and_authoritative_miss",
        "c11_bunker_url_and_nsec_creation_complete_via_actions",
        "c12_account_switch_rebinds_views_without_imperative_dance",
        "c13_view_payload_uses_placeholders_then_refines_in_place",
    ];

    // 3. Assert every row in the contract table has a matching expected test name.
    for row in &contract.rows {
        assert!(
            EXPECTED_TESTS.contains(&row.test_name.as_str()),
            "contract row {} has test name '{}' which is not in EXPECTED_TESTS — \
             update either the doc table or EXPECTED_TESTS so they agree",
            row.id, row.test_name,
        );
    }

    // 4. Assert no expected test name is missing from the contract table.
    for expected in EXPECTED_TESTS {
        let found = contract.rows.iter().any(|r| r.test_name == *expected);
        assert!(found, "EXPECTED_TESTS lists '{}' which is not in the contract doc table", expected);
    }

    // 5. Assert every EXPECTED_TESTS entry is actually a #[test] fn in this binary.
    //    Compile-time check via inventory crate or a build script that scans the file.
    for expected in EXPECTED_TESTS {
        assert!(
            test_exists_in_binary(expected),
            "EXPECTED_TESTS lists '{}' but no #[test] fn with that name exists in framework_magic_contract.rs",
            expected,
        );
    }
}
```

The meta-test is **not** `#[ignore]`. It runs on every CI run. It catches three classes of drift:

1. The doc table grows a row but the test file doesn't grow a `#[test] fn` — caught by step 4.
2. The test file grows a `#[test] fn` but the doc table doesn't list it — caught by step 3.
3. A renamed test breaks the doc-test correspondence — caught by either step 3 or 4 depending on which side renamed first.

The meta-test does **not** check `#[ignore]` status. A test for a pending milestone is correctly `#[ignore]`'d; the meta-test's job is structural correspondence, not implementation readiness. The milestone delta protocol (`intro.md` §4) handles the un-ignore cadence.

## 5. `#[ignore]` discipline

A test is `#[ignore = "pending M_n"]`'d when its owning milestone has not landed the implementation. The ignore reason **must** name the milestone (e.g., `"pending M3 tombstone persistence"`) so that a `grep -n "pending M" framework_magic_contract.rs` produces a checklist for each milestone's exit gate.

The framework-magic delta in a milestone's exit-gate report enumerates which `pending M_n` ignore lines were removed during the milestone. Removing an ignore line without the delta entry fails the post-merge codex review.

CI runs `cargo test --include-ignored` on a nightly schedule (not blocking) to catch the inverse drift: a `#[ignore]`'d test that has secretly started passing because the implementation landed without the milestone owner noticing.

## 6. Why this harness, not the existing planner harness

The M2 `PlannerHarness` (`subscription-compilation/tests.md` §9.3) is deliberately scoped to the planner subsystem: it tests the compile function in isolation, with no actor, no FFI seam, no storage backend. Several framework-magic bullets (C7 publish flow, C11 onboarding, C12 account switch, C13 in-place refinement) require the full actor and the full FFI emission path. The `ContractHarness` is therefore a strict superset.

The two harnesses live alongside each other. M2-specific tests continue to use `PlannerHarness`; cross-cutting framework-magic tests use `ContractHarness`. No deduplication is forced; an assertion's logic may exist in both files (e.g., "kind:10002 arrival triggers recompile") — the planner version asserts the compile output, the contract version asserts the wire frame plus the view payload re-emit.

## 7. Reverse-cross-reference: which milestone touches which test?

| Milestone | Tests that flip from `#[ignore]` to active |
|---|---|
| M2 | C5, C6, C8 (all sub-paths); C7 sub-paths 3 + 4 (planner-only); C13 sub-paths 2 + 3 + 4 (projection cache) |
| M3 | C2, C3, C4 (LMDB + tombstones + persistence); C9 (provenance schema + cap) |
| M4 | C10 (full sync engine) |
| M5 | (no contract bullets directly; auth-paused relays are an internal mechanism) |
| M6 | C7 sub-paths 1 + 2 + 5 (SendNote consumer); C11 (signers + onboarding actions) |
| M8 | C12 (multi-account state machine) |

Total: 13 behavior tests + 1 meta-test = 14 `#[test] fn` declarations across six milestone exit-gate transitions. The framework-magic delta at each milestone removes a known subset of `#[ignore]` lines; the contract document's "Milestone owner" column is the canonical source for which.

## 8. What this scaffolding does not specify

- **The harness implementation.** The skeleton above is the API; the implementation is the next agent's deliverable (a `framework-magic-harness` task, or the M2 milestone implementation owner folding it in).
- **The reverse mapping from `AppAction` variants to action-ledger rows.** That's `kernel-substrate.md` §4 territory; the harness exposes `action_ledger()` and the test reads rows by index/id.
- **Per-platform binding tests.** Cross-platform consistency (`subsystems.md` §3.5) is a separate test suite that runs the same scripted actions on iOS / Android / Desktop / Web and diffs `AppState` JSON. The framework-magic contract is Rust-only; platform-binding regressions show up in the cross-platform suite.
- **Negative tests for the API surface.** "The app cannot type `SendNote { content, relays: vec![...] }`" is a *compile-fail* test, owned by `docs/design/subscription-compilation/tests.md` §9.2 assertion 1. The framework-magic surface assertion is "no test passes the broken usage"; the structural inability is asserted there.
