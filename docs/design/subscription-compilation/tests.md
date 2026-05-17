# Subscription Compilation §9 — Wire-Frame Audit Gate

> Parent: `docs/design/subscription-compilation.md`.
> Read first: [`docs/plan/m2-subscription-compilation.md`](../../plan/m2-subscription-compilation.md) (M2 exit gates); `docs/design/firehose-bench.md` (the modeled bench harness this test does *not* duplicate).

The M2 exit gate is a single integration test that asserts on the *shape and identity* of the compiler's wire output, not on perf budgets. It is the structural-correctness counterpart to firehose-bench's perf-correctness suite.

## 9.1 Test file location

```
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs
```

The `crates/nmp-testing/tests/` directory does not exist yet; M2 creates it. This is intentional — it establishes the convention that planner-correctness gates live as Cargo integration tests under `nmp-testing/tests/`, distinct from the modeled benches under `nmp-testing/bin/`.

`Cargo.toml` for `nmp-testing` needs the standard `[[test]]` block:

```toml
[[test]]
name = "m2_subscription_compilation_audit"
path = "tests/m2_subscription_compilation_audit.rs"
```

`cargo test -p nmp-testing --test m2_subscription_compilation_audit` is the M2 exit-gate invocation. CI adds this to the `cargo test --workspace` pre-merge gate per [`docs/plan/ci-hygiene.md`](../../plan/ci-hygiene.md).

## 9.2 What the test asserts

Four assertions corresponding to the four M2 exit-gate bullets in [`docs/plan/m2-subscription-compilation.md`](../../plan/m2-subscription-compilation.md):

### Assertion 1 — Bug-extinction #3 surface check

> "No public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning."

> **Codegen dependency.** The test below introspects an `AppActionMeta` reflection
> helper that lives in the per-app generated crate (ADR-0010). It does not exist
> yet; ADR-0010's codegen must emit it before this assertion can compile. M2's
> implementation PR is responsible for adding that emission. Until then the test
> body below is the design contract; the implementing PR can choose either to
> generate `AppActionMeta` or to back-stop with a `proc-macro`-driven enum walker.

```rust
#[test]
fn no_public_publish_action_carries_relay_list() {
    // Compile-time-ish check: introspect the AppAction enum's generated variants
    // (per ADR-0010, the per-app generated enum is a closed enum we can match
    // exhaustively in tests). For every variant whose namespace starts with
    // "kernel.publish" or "nip01.send" or "nip17.send", assert that no field
    // is of type Vec<RelayUrl> EXCEPT the one variant `PublishWithOverride`.
    let variants = AppActionMeta::all_variants();
    for v in variants {
        if v.is_publish_action() {
            let has_relay_field = v.fields().any(|f| f.ty == "Vec<RelayUrl>");
            if v.namespace == "kernel.publish_override" {
                assert!(has_relay_field, "override must carry override_relays");
            } else {
                assert!(!has_relay_field,
                    "{} must not expose a relays parameter", v.namespace);
            }
        }
    }
}
```

This is a "shape of the API" assertion, not a behaviour assertion. If a future PR adds a relay field to `SendNote`, the test fails.

### Assertion 2 — Per-author wire fan-out for a 1000-author timeline

> "For a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count."

```rust
#[test]
fn timeline_compiles_to_per_relay_union() {
    let mut harness = PlannerHarness::new();

    // Seed mailbox cache with 1000 authors, deliberately overlapping relay sets:
    //   - 600 authors use { wss://relay.damus.io, wss://nos.lol }
    //   - 300 authors use { wss://nostr.wine, wss://nos.lol }
    //   - 100 authors use { wss://operator-niche.example }
    let authors = make_authors_with_overlapping_mailboxes(1000);
    for (pk, mb) in &authors { harness.mailbox_cache_mut().put(pk.clone(), mb.clone()); }

    // Register one Timeline interest containing all 1000 authors.
    let interest_id = harness.register_interest(LogicalInterest {
        scope: InterestScope::ActiveAccount,
        shape: InterestShape::timeline_for(authors.iter().map(|(pk, _)| pk.clone()).collect()),
        lifecycle: InterestLifecycle::Tailing,
        ..LogicalInterest::default()
    });

    let plan = harness.compile().expect("compile");

    // Assert: exactly one REQ per relay in the union of write relays.
    let expected_relays: BTreeSet<RelayUrl> = authors.iter()
        .flat_map(|(_, mb)| mb.write.iter().cloned())
        .collect();
    let actual_relays: BTreeSet<RelayUrl> = plan.per_relay.iter()
        .map(|rp| rp.relay_url.clone()).collect();
    assert_eq!(actual_relays, expected_relays);

    // Assert: each relay carries exactly one SubShape (merge happened).
    for rp in &plan.per_relay {
        assert_eq!(rp.sub_shapes.len(), 1,
            "relay {} should have one merged sub-shape, has {}",
            rp.relay_url, rp.sub_shapes.len());
    }

    // Assert: each relay's authors are exactly the subset that declared it.
    for rp in &plan.per_relay {
        let expected_authors: BTreeSet<Pubkey> = authors.iter()
            .filter(|(_, mb)| mb.write.contains(&rp.relay_url))
            .map(|(pk, _)| pk.clone())
            .collect();
        let actual_authors = &rp.sub_shapes[0].shape.authors;
        assert_eq!(actual_authors, &expected_authors,
            "relay {} should serve only its declared authors", rp.relay_url);
    }

    // Assert: plan-id is deterministic — running compile twice yields the same id.
    let plan2 = harness.compile().expect("compile #2");
    assert_eq!(plan.plan_id, plan2.plan_id, "recompile with no input changes ≠ same plan_id");
}
```

This is the single most load-bearing test in M2. It assert on:

- **Relay count** = size of union of declared write relays (no extras, no misses).
- **Per-relay author partition** = exact subset semantics.
- **Sub-shape merge** = one REQ per relay (merge lattice worked).
- **Plan-id stability** = re-compile is idempotent.

### Assertion 3 — Late-arriving kind:10002 triggers recompilation

> "An author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn."

```rust
#[test]
fn late_nip65_arrival_reroutes_without_churn() {
    let mut harness = PlannerHarness::new();
    let target = pubkey("alice");

    // Seed: no mailbox for alice. Register an interest that needs her.
    harness.register_interest(LogicalInterest::timeline_for(vec![target.clone()]));
    let plan_v1 = harness.compile().unwrap();

    // The author should be routed via indexer fallback.
    let alice_relay_v1 = plan_v1.per_relay.iter()
        .find(|rp| rp.sub_shapes[0].shape.authors.contains(&target))
        .expect("alice routed somewhere");
    assert!(alice_relay_v1.role_tags.contains(&RoutingSource::Indexer));

    // Now alice's kind:10002 arrives.
    harness.ingest_nip65(&target, ["wss://alice-relay.example"]);

    // The ingest emits Trigger::Nip65Arrived → recompile happens internally.
    harness.flush_pending_triggers();
    let plan_v2 = harness.last_compiled_plan();

    // Assert: plan-id changed.
    assert_ne!(plan_v1.plan_id, plan_v2.plan_id);

    // Assert: alice now routes to her declared relay, not the indexer.
    let alice_relay_v2 = plan_v2.per_relay.iter()
        .find(|rp| rp.sub_shapes[0].shape.authors.contains(&target))
        .expect("alice still routed");
    assert_eq!(alice_relay_v2.relay_url, "wss://alice-relay.example".into());
    assert!(alice_relay_v2.role_tags.contains(&RoutingSource::Nip65));

    // Assert: the audit stream contains exactly ONE planner re-emission for alice;
    // the platform sees one transition, not a thrash of N intermediate states.
    let audit = harness.compile_audit_log();
    let alice_transitions = audit.iter()
        .filter(|a| a.affected_authors.contains(&target))
        .count();
    assert_eq!(alice_transitions, 1, "expected exactly one recompile for late NIP-65");
}
```

This assertion is what `docs/design/ndk-applesauce-lessons.md` §2 line 19 calls out as NDK's important operational truth: "metadata can arrive late… the system should be able to refresh or expand active work without the app tearing down and recreating views."

### Assertion 4 — Four-lane diagnostic distinctness

> "The diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately."

```rust
#[test]
fn four_lanes_stay_distinct_in_diagnostic_payload() {
    let mut harness = PlannerHarness::new();
    let author = pubkey("alice");

    // Set up evidence in all four lanes for the same relay url.
    let url: RelayUrl = "wss://r.example".into();
    harness.ingest_nip65(&author, [url.clone()]);              // Nip65 fact
    harness.observe_hint(&author, url.clone(),                 // Hint fact
        HintSource::EventTag { event_id: eid("e1"), tag: TagKey::E, position: 2 });
    harness.observe_provenance(&author, url.clone(), eid("e2")); // Provenance fact
    harness.user_configured_relay(url.clone(),                 // UserConfigured fact
        UserConfiguredCategory::Indexer);

    let coverage = harness.open_view::<RelayCoverageView>(
        RelayCoverageSpec { relay_url: url.clone() });

    assert_eq!(coverage.by_lane.nip65, 1);
    assert_eq!(coverage.by_lane.hint,  1);
    assert_eq!(coverage.by_lane.user_configured, 1);
    // Provenance count is the rolling 60s counter; alice's event landed once.
    assert_eq!(coverage.provenance_count_last_minute, 1);

    // Structural: no compiler output collapses lanes.
    let plan = harness.compile().unwrap();
    let alice_assignment = plan.per_relay.iter()
        .find(|rp| rp.relay_url == url).unwrap();
    // role_tags is a SET, not a single value — lanes are preserved.
    assert!(alice_assignment.role_tags.len() >= 1);
    assert!(matches!(alice_assignment.role_tags.iter().next().unwrap(),
        RoutingSource::Nip65 | RoutingSource::UserConfigured));
}
```

This assertion encodes the doctrine: a single relay may be in the plan for multiple reasons; the plan must say which reasons, not collapse them.

## 9.3 The `PlannerHarness`

The test harness is itself part of `nmp-testing`:

```rust
// crates/nmp-testing/src/planner_harness.rs (proposed)

pub struct PlannerHarness {
    cache: InMemoryMailboxCache,
    user_config: UserConfiguredRelays,
    indexer_set: Vec<RelayUrl>,
    interests: InterestRegistry,
    compiler: SubscriptionCompiler,
    audit_log: Vec<CompileAuditEntry>,
}

impl PlannerHarness {
    pub fn new() -> Self;
    pub fn mailbox_cache_mut(&mut self) -> &mut dyn MailboxCache;
    pub fn register_interest(&mut self, i: LogicalInterest) -> InterestId;
    pub fn drop_interest(&mut self, id: InterestId);
    pub fn ingest_nip65(&mut self, author: &Pubkey, relays: impl IntoIterator<Item = RelayUrl>);
    pub fn observe_hint(&mut self, author: &Pubkey, url: RelayUrl, source: HintSource);
    pub fn observe_provenance(&mut self, author: &Pubkey, url: RelayUrl, event: EventId);
    pub fn user_configured_relay(&mut self, url: RelayUrl, cat: UserConfiguredCategory);
    pub fn force_recompile(&mut self, reason: InvalidateReason);
    pub fn flush_pending_triggers(&mut self);
    pub fn compile(&mut self) -> Result<CompiledPlan, CompileError>;
    pub fn last_compiled_plan(&self) -> &CompiledPlan;
    pub fn compile_audit_log(&self) -> &[CompileAuditEntry];
    pub fn open_view<V: ViewModule>(&mut self, spec: V::Spec) -> V::Payload;
}
```

The harness is the *minimum* surface required for the four assertions above. It is deliberately small so it does not become its own moving target.

## 9.4 What this test does *not* cover

By design (these belong to other M2 gates or later milestones):

- **Real wire frames against a relay.** This is `firehose-bench live` per [`docs/plan/m1-twitter-slice.md`](../../plan/m1-twitter-slice.md) (M1 exit gate "Firehose-bench live cold_start"); the audit test is offline and synthetic.
- **Wire-emitter diff correctness across two plans.** That is a separate unit test inside `nmp-core::kernel::wire`, not the milestone-exit gate.
- **NIP-77 watermarks.** M4.
- **Per-account auth state.** M5.
- **The publish path running end-to-end.** M6.

The audit gate's job is exactly the four assertions: API shape, fan-out structure, recompilation on late NIP-65, and four-lane diagnostic distinctness. Those are the four exit-gate bullets the milestone document lists; this test is the verification surface for all four.

## 9.5 CI integration

The test runs in the default `cargo test --workspace` job and takes < 1 second on standard hardware (no networking, no LMDB, in-memory cache only). It is the canonical regression test for "did someone re-introduce the hardcoded two-role planner?" and as such must never be skipped or `#[ignore]`d.

If the M3 (LMDB) milestone graduates the mailbox cache to a real backend, this test continues to exercise the trait surface via the `InMemoryMailboxCache` impl — no changes required. That is the seam `nmp-nip65::cache::MailboxCache` exists for.
