//! Case C: `#p` tag values → Inbox.
//!
//! Structural ban: `#p` interests MUST route to the inbox relay set selected
//! by the interest. Generic `#p` interests use NIP-65 read relays; NIP-17
//! gift-wrap inbox interests use kind:10050 DM relays. We never route to the
//! author's write relays, and we do not fall back to the indexer set.
//!
//! When inbox relays are unknown, we emit NO relay entries (fail-closed) and
//! emit a probe so the next recompile has data. The plan will have an empty
//! `per_relay` map for this interest until kind:10002 or kind:10050 arrives.
//!
//! ## PD-033-C planner extension (precursor to Stage 2 — `#p` bootstrap)
//!
//! The sibling [`route_bootstrap_content_inbox`] helper handles the cold-start
//! case where the kernel-driven self-zap-receipts subscription
//! (`kernel/requests/startup.rs`: `kind:9735 #p=[self_pk]` on
//! `RelayRole::Content`) needs to fly BEFORE the active account's kind:10002
//! arrives. Pre-PD-033-C the M1 `req(Content, …)` helper unconditionally
//! emitted the REQ on `bootstrap_urls_for_role(RelayRole::Content)`; the
//! planner mirror routes the equivalent `LogicalInterest` shape to
//! `bootstrap_content_relays` exactly when:
//!
//! - `lifecycle == Tailing`
//! - `scope == Global`
//! - `p_tag_routing == Nip65ReadRelays` (NIP-17 DM relays remain fail-closed
//!   by design — gift-wraps must NEVER leak to a non-DM relay)
//! - EVERY tagged pubkey has NO NIP-65 inbox cached (`get(pk)` is `None`, or
//!   the snapshot's `has_inbox_relays()` returns `false`)
//! - `bootstrap_content_relays` is non-empty
//!
//! When kind:10002 later arrives for any of the tagged pubkeys, the next
//! recompile naturally re-routes (the gate evaluates false because at least
//! one pubkey now has an inbox), and `plan_diff` emits a CLOSE on the
//! bootstrap relay paired with a REQ on the real inbox relay. No per-pubkey
//! narrowing on the bootstrap landing — the bootstrap content relay is a
//! single cold-start pad, not a per-recipient mailbox, so the original `#p`
//! set is preserved verbatim.
//!
//! The gate is the dispatcher's responsibility ([`super::partition_interest`]);
//! this helper assumes the gate already fired.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2,
//!          `docs/architecture-audit/pd033c-plan.md` §4.3
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestId, InterestLifecycle, InterestShape, LogicalInterest, Pubkey, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};
use super::{MailboxCache, RelayEntry};
use super::inbox_helper::route_p_tags_to_inbox;

/// Route a `#p`-only interest (no authors/addresses) to inbox relays.
///
/// Passes an empty `authors_for_inbox` set because there is no author
/// constraint — the interest matches any event tagging the specified pubkeys.
/// The per-pubkey `#p` scoping in `route_p_tags_to_inbox` still applies:
/// Bob's relay sees only `#p:[Bob]`, not the full set of tagged pubkeys.
pub(super) fn route(
    p_tag_values: &BTreeSet<Pubkey>,
    base_shape: &InterestShape,
    lifecycle: &InterestLifecycle,
    interest_id: &InterestId,
    mailbox_cache: &dyn MailboxCache,
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    // No `authors` (the Case A guard ensures this) — pass an empty set so
    // the inbox shape doesn't constrain authors.
    let empty_authors: BTreeSet<Pubkey> = BTreeSet::new();
    route_p_tags_to_inbox(
        p_tag_values,
        &empty_authors,
        base_shape,
        lifecycle,
        interest_id,
        mailbox_cache,
        relay_entries,
    );
}

/// PD-033-C planner extension: route a `Tailing + Global + #p` interest to
/// `bootstrap_content_relays` when every tagged pubkey lacks a cached
/// NIP-65 inbox.
///
/// Mirrors M1's `req(RelayRole::Content, …)` cold-start emission for the
/// kernel's self-zap-receipts subscription (`kind:9735 #p=[self_pk]`,
/// `kernel/requests/startup.rs`). Without this, deleting the M1 helper would
/// silently lose every #p-tagged Tailing REQ until kind:10002 arrives —
/// breaking the F-04 zap-receipts contract on cold-start sign-ins.
///
/// Gating happens at the dispatcher ([`super::partition_interest`]); this
/// helper assumes the caller has already verified:
/// - `lifecycle == Tailing` AND `scope == Global`
/// - `p_tag_routing == Nip65ReadRelays` (NIP-17 DM relays must NEVER divert
///   to a non-DM relay)
/// - every tagged pubkey has no cached NIP-65 inbox
/// - `bootstrap_content_relays` is non-empty
///
/// All emitted entries are tagged
/// `RoutingSource::UserConfigured(UserConfiguredCategory::Bootstrap)` so
/// diagnostics distinguish cold-start bootstrap routing from regular inbox
/// routing once mailboxes arrive.
///
/// The original `#p` tag set is preserved verbatim in `base_shape.tags` —
/// no per-pubkey narrowing happens on the bootstrap landing because the
/// bootstrap relay is a shared cold-start pad, not a per-recipient mailbox.
///
/// Signature mirrors [`super::case_d_no_author::route_bootstrap_content`]
/// (the Stage 1 sibling) verbatim: `&LogicalInterest` in, four-lane relay
/// accumulator out — no intermediate wrapper types. Symmetry with the
/// existing helper is the readability invariant.
pub(super) fn route_bootstrap_content_inbox(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    bootstrap_content_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    let mut per_relay: BTreeMap<RelayUrl, BTreeSet<RoutingSource>> = BTreeMap::new();
    for relay in bootstrap_content_relays {
        per_relay
            .entry(relay.clone())
            .or_default()
            .insert(RoutingSource::UserConfigured(UserConfiguredCategory::Bootstrap));
    }
    for (relay_url, sources) in per_relay {
        relay_entries.entry(relay_url).or_default().push(RelayEntry {
            base_shape: base_shape.clone(),
            authors_for_relay: BTreeSet::new(),
            addresses_for_relay: BTreeSet::new(),
            lifecycle: interest.lifecycle.clone(),
            sources,
            interest_id: interest.id.clone(),
        });
    }
}

/// Predicate: every pubkey in `p_tag_values` has NO cached NIP-65 inbox.
///
/// The gate's pre-condition for [`route_bootstrap_content_inbox`]. Returns
/// `true` when EVERY tagged pubkey's `mailbox_cache.get(pk)` is `None` OR the
/// snapshot's `has_inbox_relays()` returns `false`. If ANY tagged pubkey has
/// an inbox cached, the regular `route` path can serve at least one recipient
/// and the bootstrap fallback must NOT fire (would over-fetch by routing to
/// both the real inbox AND the bootstrap relay).
///
/// Empty `p_tag_values` returns `true` vacuously, but the dispatcher's Case C
/// guard rules out that branch before either helper is called.
pub(super) fn every_tagged_pubkey_lacks_nip65_inbox(
    p_tag_values: &BTreeSet<Pubkey>,
    mailbox_cache: &dyn MailboxCache,
) -> bool {
    p_tag_values.iter().all(|pk| match mailbox_cache.get(pk) {
        Some(snapshot) => !snapshot.has_inbox_relays(),
        None => true,
    })
}

#[cfg(test)]
mod tests {
    //! PD-033-C planner extension — Case C bootstrap-content inbox fallback.
    //!
    //! Mirrors the matrix in `case_d_no_author.rs::pd033c_*` (Stage 1
    //! precedent): positive route, scope=Account counterpoint, lifecycle=OneShot
    //! counterpoint, p_tag_routing=Nip17DmRelays counterpoint (fail-closed
    //! preserved), partial inbox cache counterpoint (gate refuses), empty
    //! bootstrap counterpoint (fall through to fail-closed), and plan_id
    //! stability under bootstrap toggle.
    //!
    //! The headline contract: a `Tailing + Global + #p (Nip65ReadRelays)`
    //! interest whose tagged pubkey has no cached NIP-65 inbox AND
    //! `bootstrap_content_relays` is non-empty routes to the bootstrap content
    //! lane, lane = `UserConfigured(Bootstrap)`. This is the silent-loss
    //! regression Stage 2 of PD-033-C exposes for the kernel's self-zap-receipts
    //! subscription (`kind:9735 #p=[self_pk]` on `RelayRole::Content`).
    use crate::planner::{
        compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
        interest::{
            InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
            PTagRouting,
        },
        plan::{RoutingSource, UserConfiguredCategory},
    };
    use std::collections::{BTreeMap, BTreeSet};

    /// Deterministic 64-char hex pubkey fixture from a short label.
    fn pk(s: &str) -> String {
        format!("{s:0>64}").chars().take(64).collect()
    }

    /// Build a `#p`-only interest with the given `p_tag_routing` mode.
    /// Defaults to kind:9735 (the self-zap-receipts shape) and the canonical
    /// `Tailing + Global` lifecycle/scope that the dispatcher gate keys on.
    fn p_tag_interest(
        id: u64,
        tagged: &[&str],
        routing: PTagRouting,
        lifecycle: InterestLifecycle,
        scope: InterestScope,
    ) -> LogicalInterest {
        let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let values: BTreeSet<String> = tagged.iter().map(|p| pk(p)).collect();
        tags.insert("p".to_string(), values);
        LogicalInterest {
            id: InterestId(id),
            scope,
            shape: InterestShape {
                kinds: [9735u32].into_iter().collect(),
                tags,
                limit: Some(50),
                p_tag_routing: routing,
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle,
        }
    }

    fn self_zap_receipts_interest() -> LogicalInterest {
        p_tag_interest(
            1,
            &["self"],
            PTagRouting::Nip65ReadRelays,
            InterestLifecycle::Tailing,
            InterestScope::Global,
        )
    }

    // ── PD-033-C — bootstrap inbox lane (§4.3 — Stage 2 precursor) ──────────

    /// Headline routing decision: a `Tailing + Global + #p (Nip65ReadRelays)`
    /// interest whose tagged pubkey has NO cached NIP-65 inbox AND
    /// `bootstrap_content_relays` is non-empty routes to the bootstrap content
    /// lane (lane `UserConfigured(Bootstrap)`). This is the silent-loss
    /// regression Stage 2 exposes for the kernel's self-zap-receipts subscription
    /// — without this gate, deleting the M1 `req(Content, …)` helper would lose
    /// every #p-tagged Tailing REQ until kind:10002 lands.
    #[test]
    fn pd033c_p_tag_tailing_global_no_inbox_routes_to_bootstrap_content() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap_content = vec!["wss://relay.primal.net".to_string()];
        // Active-account / app / indexer relays present to prove the gate
        // chooses BOOTSTRAP specifically, not any of those (all wrong for the
        // self-zap-receipts cold-start: indexer is discovery-only, AccountRead
        // is for hashtag firehose, AppRelay rides Case A not Case C).
        let indexer = vec!["wss://purplepag.es".to_string()];
        let aar = vec!["wss://user-read.example".to_string()];
        let app = vec!["wss://user-app.example".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &indexer,
            &aar,
            &app,
            &bootstrap_content,
            /* bootstrap_indexer = */ &[],
        );

        let plan = compiler
            .compile(&[self_zap_receipts_interest()])
            .expect("compile");

        let landed = plan
            .per_relay
            .get("wss://relay.primal.net")
            .expect("bootstrap content relay must carry the #p Tailing REQ");
        assert!(
            landed.role_tags.contains(&RoutingSource::UserConfigured(
                UserConfiguredCategory::Bootstrap
            )),
            "bootstrap content lane must be recorded; got role_tags = {:?}",
            landed.role_tags
        );
        // Exactly one relay served the REQ — none of the other configured
        // relays may carry it (the gate is exclusive, not additive).
        assert_eq!(plan.per_relay.len(), 1);
        assert!(plan.per_relay.get("wss://purplepag.es").is_none());
        assert!(plan.per_relay.get("wss://user-read.example").is_none());
        assert!(plan.per_relay.get("wss://user-app.example").is_none());
    }

    /// Once kind:10002 arrives for the tagged pubkey, the next recompile re-
    /// routes off the bootstrap content lane onto the real inbox relays. This
    /// is the load-bearing transition that proves the gate is dynamic — a
    /// stuck-on-bootstrap regression would cap zap-receipt delivery to the
    /// cold-start lane forever.
    #[test]
    fn pd033c_p_tag_routes_off_bootstrap_when_inbox_arrives() {
        let bootstrap_content = vec!["wss://bootstrap.example".to_string()];

        // Phase 1: no inbox cached → bootstrap.
        let empty_cache = InMemoryMailboxCache::new();
        let before = SubscriptionCompiler::with_relays_and_bootstrap(
            &empty_cache,
            &[],
            &[],
            &[],
            &bootstrap_content,
            &[],
        )
        .compile(&[self_zap_receipts_interest()])
        .expect("compile");
        assert!(
            before.per_relay.contains_key("wss://bootstrap.example"),
            "phase 1: bootstrap carries the #p REQ when no inbox cached"
        );

        // Phase 2: kind:10002 arrives → the same interest re-routes to the
        // real inbox relay and the bootstrap lane is no longer used.
        let mut after_cache = InMemoryMailboxCache::new();
        after_cache.put(
            pk("self"),
            MailboxSnapshot {
                write_relays: vec![],
                read_relays: vec!["wss://self-read.example".to_string()],
                both_relays: vec![],
            },
        );
        let after = SubscriptionCompiler::with_relays_and_bootstrap(
            &after_cache,
            &[],
            &[],
            &[],
            &bootstrap_content,
            &[],
        )
        .compile(&[self_zap_receipts_interest()])
        .expect("compile");
        assert!(
            after.per_relay.contains_key("wss://self-read.example"),
            "phase 2: real inbox carries the #p REQ once kind:10002 lands"
        );
        assert!(
            after.per_relay.get("wss://bootstrap.example").is_none(),
            "phase 2: bootstrap lane MUST be retired when an inbox is cached \
             (gate evaluates false)"
        );
    }

    /// Counterpoint — empty `bootstrap_content_relays` falls through to the
    /// existing Case C body (fail-closed) and emits ZERO relay entries. Proves
    /// the gate is a strict superset opt-in.
    #[test]
    fn pd033c_p_tag_empty_bootstrap_falls_through_to_fail_closed() {
        let cache = InMemoryMailboxCache::new();
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            /* bootstrap_content = */ &[],
            &[],
        );

        let plan = compiler
            .compile(&[self_zap_receipts_interest()])
            .expect("compile");

        assert!(
            plan.per_relay.is_empty(),
            "Case C fail-closed semantics preserved when bootstrap is empty; \
             got per_relay = {:?}",
            plan.per_relay.keys().collect::<Vec<_>>()
        );
    }

    /// Counterpoint — `OneShot + Global + #p` does NOT trigger the gate. The
    /// gate is keyed on Tailing specifically (the self-zap-receipts shape;
    /// OneShot would imply a one-time inbox probe, which is not a defined
    /// kernel path today). A future OneShot+#p caller would need its own
    /// explicit gate.
    #[test]
    fn pd033c_p_tag_oneshot_does_not_trigger_gate() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &bootstrap_content,
            &[],
        );

        let interest = p_tag_interest(
            1,
            &["self"],
            PTagRouting::Nip65ReadRelays,
            InterestLifecycle::OneShot,
            InterestScope::Global,
        );

        let plan = compiler.compile(&[interest]).expect("compile");
        assert!(
            plan.per_relay.get("wss://bootstrap.example").is_none(),
            "OneShot + #p must NOT trigger the bootstrap inbox gate (the gate \
             is scoped to the Tailing self-zap-receipts shape)"
        );
        // Falls through to the regular Case C path which fail-closes when
        // inbox is unknown.
        assert!(plan.per_relay.is_empty());
    }

    /// Counterpoint — `Tailing + Account(x) + #p` does NOT trigger the gate.
    /// Account-scoped #p interests have an explicit account context and should
    /// route via that account's inbox (or fail-closed) — diverting them to a
    /// shared cold-start lane would mix multi-account contexts on one relay.
    #[test]
    fn pd033c_p_tag_account_scoped_does_not_trigger_gate() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &bootstrap_content,
            &[],
        );

        let interest = p_tag_interest(
            1,
            &["self"],
            PTagRouting::Nip65ReadRelays,
            InterestLifecycle::Tailing,
            InterestScope::Account(pk("alice")),
        );

        let plan = compiler.compile(&[interest]).expect("compile");
        assert!(
            plan.per_relay.get("wss://bootstrap.example").is_none(),
            "Account-scoped #p must NOT divert to the bootstrap content lane"
        );
    }

    /// Counterpoint — `Tailing + Global + #p (Nip17DmRelays)` MUST stay
    /// fail-closed. NIP-17 gift-wrapped DMs are private; diverting them to a
    /// non-DM relay would leak gift-wraps to a relay the recipient never
    /// authorised. This counterpoint locks the privacy-critical exclusion.
    #[test]
    fn pd033c_p_tag_nip17_dm_routing_stays_fail_closed() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &bootstrap_content,
            &[],
        );

        let interest = p_tag_interest(
            1,
            &["self"],
            PTagRouting::Nip17DmRelays,
            InterestLifecycle::Tailing,
            InterestScope::Global,
        );

        let plan = compiler.compile(&[interest]).expect("compile");
        assert!(
            plan.per_relay.is_empty(),
            "NIP-17 DM routing MUST stay fail-closed when DM relays are \
             unknown — diverting gift-wraps to a non-DM relay would leak \
             private DMs. Got per_relay = {:?}",
            plan.per_relay.keys().collect::<Vec<_>>()
        );
    }

    /// Counterpoint — when ANY tagged pubkey has a cached NIP-65 inbox, the
    /// gate refuses and the regular Case C body fires for all pubkeys. The
    /// bootstrap fallback must NOT additively double-route (would
    /// over-subscribe the bootstrap relay).
    #[test]
    fn pd033c_p_tag_partial_inbox_cache_does_not_trigger_gate() {
        let mut cache = InMemoryMailboxCache::new();
        // Bob has a cached inbox; Carol does not.
        cache.put(
            pk("bob"),
            MailboxSnapshot {
                write_relays: vec![],
                read_relays: vec!["wss://bob-read.example".to_string()],
                both_relays: vec![],
            },
        );
        let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &bootstrap_content,
            &[],
        );

        let interest = p_tag_interest(
            1,
            &["bob", "carol"],
            PTagRouting::Nip65ReadRelays,
            InterestLifecycle::Tailing,
            InterestScope::Global,
        );

        let plan = compiler.compile(&[interest]).expect("compile");
        // Bob's inbox carries his #p shard via the regular Case C body.
        assert!(
            plan.per_relay.get("wss://bob-read.example").is_some(),
            "Bob's NIP-65 inbox must carry his #p shard via the regular \
             Case C body"
        );
        // Bootstrap content MUST NOT be touched — partial cache disables
        // the gate so the regular fail-closed semantics apply to Carol.
        assert!(
            plan.per_relay.get("wss://bootstrap.example").is_none(),
            "partial inbox cache must DISABLE the bootstrap fallback (the \
             gate is all-or-nothing); got per_relay = {:?}",
            plan.per_relay.keys().collect::<Vec<_>>()
        );
    }

    /// Counterpoint — a tagged pubkey with a cached snapshot whose
    /// `has_inbox_relays()` returns `false` (an empty kind:10002 declared
    /// zero read relays) IS treated as "no inbox" by the gate, exactly as
    /// the "no snapshot at all" case. Pins the predicate semantics.
    #[test]
    fn pd033c_p_tag_empty_inbox_snapshot_treated_as_no_inbox() {
        let mut cache = InMemoryMailboxCache::new();
        // An author whose kind:10002 declared write relays but zero read
        // relays. Per NIP-65 the snapshot exists but `has_inbox_relays()` is
        // false. The gate must treat this as "no inbox" and divert.
        cache.put(
            pk("self"),
            MailboxSnapshot {
                write_relays: vec!["wss://self-write.example".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &bootstrap_content,
            &[],
        );

        let plan = compiler
            .compile(&[self_zap_receipts_interest()])
            .expect("compile");
        assert!(
            plan.per_relay.contains_key("wss://bootstrap.example"),
            "an empty inbox snapshot must be treated as no-inbox by the gate"
        );
    }

    /// `bootstrap_content_relays` MUST be excluded from `compute_plan_id` —
    /// toggling it at runtime must not churn sub-ids (matches the
    /// `app_relays` treatment and the existing Case D bootstrap test
    /// `pd033c_bootstrap_toggle_does_not_change_plan_id`). Without this,
    /// every kind:10002 arrival would invalidate every plan-id and trigger a
    /// spurious re-emit of every wire frame.
    #[test]
    fn pd033c_p_tag_bootstrap_toggle_does_not_change_plan_id() {
        let cache = InMemoryMailboxCache::new();
        let interests = [self_zap_receipts_interest()];

        let bootstrap_set = vec!["wss://bootstrap.example".to_string()];
        let no_bootstrap = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            /* bootstrap_content = */ &[],
            &[],
        );
        let with_bootstrap = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &bootstrap_set,
            &[],
        );

        let plan_without = no_bootstrap.compile(&interests).expect("compile");
        let plan_with = with_bootstrap.compile(&interests).expect("compile");

        // Behaviour differs — without bootstrap, fail-closed; with bootstrap,
        // routed to the cold-start lane.
        assert!(plan_without.per_relay.is_empty());
        assert!(plan_with.per_relay.contains_key("wss://bootstrap.example"));

        // But plan_id is identical — bootstrap_content_relays is excluded
        // from the hash, matching the app_relays-toggle invariant.
        assert_eq!(
            plan_without.plan_id, plan_with.plan_id,
            "bootstrap_content_relays must be excluded from compute_plan_id; \
             toggling it MUST NOT churn sub-ids"
        );
    }
}
