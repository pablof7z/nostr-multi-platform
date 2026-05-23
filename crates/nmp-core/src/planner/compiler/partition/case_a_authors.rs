//! Case A: explicit `authors` → Outbox (write relays).
//!
//! When an interest carries both `authors` AND `#p` tag values, this case
//! emits Outbox entries AND calls `inbox_helper::route_p_tags_to_inbox` for
//! the "both populated" split (spec §3.1 "Both populated" row).
//!
//! ## Routing rules (post-T134 clarification)
//!
//! - Author with known NIP-65 mailbox → REQ goes to the UNION of the author's
//!   `outbox_relays()` and the kernel-configured `app_relays`. Both lanes are
//!   recorded on the resulting `RelayEntry::sources` so diagnostics see
//!   `{Nip65, UserConfigured(AppRelay)}` for any URL that landed in both
//!   sets.
//! - Author with NO NIP-65 mailbox → REQ goes to `app_relays` ONLY, with
//!   lane `UserConfigured(AppRelay)`. We still emit `request_probe` so that
//!   kind:10002 lookup populates the mailbox cache and the next recompile
//!   routes the author through NIP-65.
//! - Author with NO NIP-65 mailbox AND no `app_relays` configured AND the
//!   interest is `OneShot + Global` → REQ goes to `bootstrap_indexer_relays`
//!   with lane `UserConfigured(Indexer)`. This is the PD-033-C planner-
//!   extension arm (`docs/architecture-audit/pd033c-plan.md` §4.3): kernel-
//!   driven discovery oneshots for referenced pubkeys
//!   (`kernel/discovery.rs::drain_unknown_oneshots`'s profile-oneshot arm) fan
//!   to `RelayRole::Indexer` for kind:0/3/10002 lookups, so the planner must
//!   mirror that decision for the equivalent `LogicalInterest`.
//!   `bootstrap_indexer_relays` is the WITH-FALLBACK form (carries
//!   `FALLBACK_INDEXER_RELAY` when no indexer row is configured yet), matching
//!   `Kernel::bootstrap_urls_for_role(RelayRole::Indexer)` byte-for-byte —
//!   crucial so cold-start sign-ins (no rows yet) don't lose discovery REQs
//!   the moment Stage 1 deletes the M1 helper. The raw `indexer_relays` field
//!   (no fallback) is INTENTIONALLY not consulted here; using it would
//!   silently disable discovery whenever the operator hadn't yet configured
//!   an indexer row.
//! - Author with NO NIP-65 mailbox AND no `app_relays` AND NOT a `OneShot +
//!   Global` interest → the author is recorded in `unroutable` so the kernel
//!   can surface a UI diagnostic. The interest still flies to other authors'
//!   relays.
//!
//! Outside the PD-033-C `OneShot + Global` arm the indexer set is NEVER
//! consulted in this case — indexers remain discovery-only for tailing
//! follow-feed authors (the T134 invariant), and the kernel surfaces missing
//! mailboxes as "unroutable" via the UI toast as before.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestScope, InterestLifecycle, InterestShape, LogicalInterest, NaddrCoord, Pubkey, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};
use super::{MailboxCache, RelayEntry};
use super::inbox_helper::route_p_tags_to_inbox;

/// Per-relay accumulator: (authors, addresses, sources).
type CaseAEntry = (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, BTreeSet<RoutingSource>);

/// Route an interest with explicit authors to outbox relays.
///
/// Also emits inbox entries for any `#p` tag values ("both populated" split).
/// The inbox slice carries the original `authors` so the REQ semantics remain
/// `authors AND #p` (intersection) rather than a wildcard over all #p events.
///
/// `bootstrap_indexer_relays` is the PD-033-C planner-extension fallback for
/// the `OneShot + Global` discovery-oneshot arm only — see module doc and the
/// `if !landed` block below. Tailing / account-scoped interests never touch
/// it. The raw `indexer_relays` field is deliberately not threaded in here
/// (cold-start divergence: `bootstrap_indexer_relays` carries
/// `FALLBACK_INDEXER_RELAY` when rows are empty; raw `indexer_relays` does
/// not).
//
// `too_many_arguments` allowed: this is a crate-internal routing helper whose
// parameters mirror the public compiler context plus its two accumulators;
// repackaging them behind a struct would obscure the dispatch in
// `partition::partition_interest` for no readability gain.
#[allow(clippy::too_many_arguments)]
pub(super) fn route(
    interest: &LogicalInterest,
    p_tag_values: &BTreeSet<Pubkey>,
    base_shape: &InterestShape,
    mailbox_cache: &dyn MailboxCache,
    app_relays: &[RelayUrl],
    bootstrap_indexer_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
    unroutable: &mut BTreeSet<Pubkey>,
) {
    // PD-033-C: gates the kernel-driven discovery-oneshot fallback. The two
    // conjuncts (`OneShot` + `Global`) intentionally match
    // `kernel/discovery.rs::drain_unknown_oneshots`'s shape exactly —
    // `oneshot.request(registry, InterestScope::Global, shape)` always
    // constructs an interest with `lifecycle: OneShot` (see
    // `subs/oneshot.rs::request`). Account-scoped profile fetches and tailing
    // follow-feed interests both fail this gate and retain their
    // pre-PD-033-C unroutable behaviour.
    let is_discovery_oneshot = matches!(interest.lifecycle, InterestLifecycle::OneShot)
        && matches!(interest.scope, InterestScope::Global);
    // Accumulate per-relay (authors, addresses, sources) before pushing
    // RelayEntry objects. This lets multiple authors share a relay without
    // creating separate entries — Stage 3 merge operates on the combined set.
    //
    // The per_relay map accumulates ALL RoutingSource lanes per relay URL.
    // Author A may reach wss://x via NIP-65 while author B reaches the same
    // relay via AppRelay (because the operator pinned the same URL); both
    // lanes must be recorded so Stage 3 role_tags reflects the four-lane
    // model (§3.1 four-lane discipline).
    let mut per_relay: BTreeMap<RelayUrl, CaseAEntry> = BTreeMap::new();

    for author in &interest.shape.authors {
        // Track whether ANY relay entry was emitted for this author. A known
        // mailbox with an empty `outbox_relays()` and no `app_relays` is just
        // as unroutable as a missing mailbox — in both cases zero REQs go out
        // for this author, which is exactly the condition the kernel surfaces
        // a toast for. The strict per-author landing-pad check captures both.
        let mut landed = false;

        match mailbox_cache.get(author) {
            Some(snapshot) => {
                // NIP-65 lane: author's declared write relays.
                for relay in snapshot.outbox_relays() {
                    let entry = per_relay
                        .entry(relay.clone())
                        .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                    entry.0.insert(author.clone());
                    entry.2.insert(RoutingSource::Nip65);
                    landed = true;
                }
            }
            None => {
                // No mailbox known — probe so cache can be populated and
                // the next recompile routes via NIP-65 (§3.2). Probing is
                // independent of whether app_relays carry this author now;
                // a NIP-65 update later moves them onto their own write set.
                mailbox_cache.request_probe(author);
            }
        }

        // AppRelay lane: additive whenever app_relays are configured, both
        // when NIP-65 is known AND when it is unknown.
        for relay in app_relays {
            let entry = per_relay
                .entry(relay.clone())
                .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
            entry.0.insert(author.clone());
            entry.2.insert(RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay));
            landed = true;
        }

        if !landed {
            // PD-033-C planner extension: a `OneShot + Global` interest whose
            // author has no NIP-65 mailbox AND no app_relays falls back to
            // `bootstrap_indexer_relays` instead of being marked unroutable.
            // This matches `kernel/discovery.rs::drain_unknown_oneshots`'s
            // profile-oneshot arm which fans the equivalent kind:0/3/10002
            // filter to `RelayRole::Indexer` today (the kernel calls
            // `bootstrap_urls_for_role(RelayRole::Indexer)`, which includes the
            // `FALLBACK_INDEXER_RELAY` cold-start default — so on cold-start
            // sign-ins the discovery REQ still lands somewhere). Tailing
            // follow-feed interests are NOT eligible — they continue to land
            // in `unroutable` so the kernel can surface the toast.
            if is_discovery_oneshot && !bootstrap_indexer_relays.is_empty() {
                for relay in bootstrap_indexer_relays {
                    let entry = per_relay
                        .entry(relay.clone())
                        .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                    entry.0.insert(author.clone());
                    entry.2.insert(RoutingSource::UserConfigured(UserConfiguredCategory::Indexer));
                    landed = true;
                }
            }
            if !landed {
                unroutable.insert(author.clone());
            }
        }
    }

    for coord in &interest.shape.addresses {
        let mut landed = false;

        match mailbox_cache.get(&coord.pubkey) {
            Some(snapshot) => {
                for relay in snapshot.outbox_relays() {
                    let entry = per_relay
                        .entry(relay.clone())
                        .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
                    entry.1.insert(coord.clone());
                    entry.2.insert(RoutingSource::Nip65);
                    landed = true;
                }
            }
            None => {
                mailbox_cache.request_probe(&coord.pubkey);
            }
        }

        for relay in app_relays {
            let entry = per_relay
                .entry(relay.clone())
                .or_insert_with(|| (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()));
            entry.1.insert(coord.clone());
            entry.2.insert(RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay));
            landed = true;
        }

        if !landed {
            // PD-033-C planner extension is intentionally NOT applied to the
            // address-pointer arm. The kernel-driven discovery oneshots in
            // `kernel/discovery.rs::drain_unknown_oneshots` only target
            // `event_ids` (content arm) and `authors` (profile arm) — never
            // `addresses`. Address-pointer hydration is a view-module
            // responsibility (e.g. `nmp_nip01::ThreadView`) and runs through
            // the regular Case A author lane via the coord's `pubkey`. Keeping
            // the unroutable behaviour here preserves the existing UI
            // diagnostic for addressable events with no NIP-65/app-relays —
            // exactly the pre-PD-033-C semantics.
            unroutable.insert(coord.pubkey.clone());
        }
    }

    for (relay_url, (authors, addrs, sources)) in per_relay {
        relay_entries.entry(relay_url).or_default().push(RelayEntry {
            base_shape: base_shape.clone(),
            authors_for_relay: authors,
            addresses_for_relay: addrs,
            lifecycle: interest.lifecycle.clone(),
            sources,
            interest_id: interest.id.clone(),
        });
    }

    // "Both populated" split: also emit Inbox entries for any #p values.
    //
    // The inbox slice preserves the original author constraint — the
    // semantics are `authors AND #p` (intersection), so the inbox REQ on
    // the tagged pubkey's read relays must still be filtered by the
    // interest's authors, not a wildcard that would match every event
    // tagging the recipient.
    if !p_tag_values.is_empty() {
        route_p_tags_to_inbox(
            p_tag_values,
            &interest.shape.authors,
            base_shape,
            &interest.lifecycle,
            &interest.id,
            mailbox_cache,
            relay_entries,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::planner::{
        compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
        interest::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest},
        plan::{RoutingSource, UserConfiguredCategory},
    };

    fn pk(s: &str) -> String {
        format!("{s:0>64}").chars().take(64).collect()
    }

    fn timeline_interest(id: u64, authors: &[&str]) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: authors.iter().map(|a| pk(a)).collect(),
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        }
    }

    /// NIP-65 known author + app_relays configured → REQ to UNION of both
    /// sets; the shared URL records BOTH lanes.
    #[test]
    fn case_a_nip65_known_unions_with_app_relays() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pk("alice"),
            MailboxSnapshot {
                write_relays: vec!["wss://alice-write".to_string(), "wss://shared".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        let indexer: Vec<String> = vec![];
        let app = vec!["wss://app".to_string(), "wss://shared".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

        let plan = compiler.compile(&[timeline_interest(1, &["alice"])]).expect("compile");

        // NIP-65 lane only on the author-only URL.
        let alice_only = plan.per_relay.get("wss://alice-write").expect("alice-write");
        assert!(alice_only.role_tags.contains(&RoutingSource::Nip65));
        assert!(!alice_only
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));

        // AppRelay lane only on the app-only URL.
        let app_only = plan.per_relay.get("wss://app").expect("app");
        assert!(app_only
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));
        assert!(!app_only.role_tags.contains(&RoutingSource::Nip65));

        // Both lanes on the shared URL.
        let shared = plan.per_relay.get("wss://shared").expect("shared");
        assert!(shared.role_tags.contains(&RoutingSource::Nip65));
        assert!(shared
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));

        // No author is unroutable.
        assert!(plan.unroutable_authors.is_empty());
    }

    /// NIP-65 unknown author + app_relays configured → REQ to app_relays
    /// ONLY (no indexer fallback), AppRelay lane.
    #[test]
    fn case_a_nip65_unknown_routes_to_app_relays_only() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let app = vec!["wss://app".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

        let plan = compiler.compile(&[timeline_interest(1, &["bob"])]).expect("compile");

        // Indexer URL is NEVER consulted for content routing now.
        assert!(plan.per_relay.get("wss://purplepag.es").is_none());

        // App relay carries Bob with the AppRelay lane only.
        let app_plan = plan.per_relay.get("wss://app").expect("app");
        assert!(app_plan
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));
        assert!(!app_plan
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Indexer)));

        // Bob is NOT unroutable — app_relays carried him.
        assert!(plan.unroutable_authors.is_empty());
    }

    /// NIP-65 unknown author + no app_relays → author lands in
    /// `unroutable_authors`; the indexer is NOT a fallback.
    #[test]
    fn case_a_no_nip65_no_app_relays_marks_author_unroutable() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let app: Vec<String> = vec![];
        let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

        let plan = compiler.compile(&[timeline_interest(1, &["bob"])]).expect("compile");

        assert!(plan.per_relay.is_empty(), "no relays should be selected for content");
        assert!(
            plan.unroutable_authors.contains(&pk("bob")),
            "bob should be marked unroutable; got {:?}",
            plan.unroutable_authors
        );
    }

    /// Multi-author: one with NIP-65, one without; with app_relays both land
    /// SOMEWHERE — neither is unroutable.
    #[test]
    fn case_a_mixed_nip65_known_and_unknown_with_app_relays() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pk("alice"),
            MailboxSnapshot {
                write_relays: vec!["wss://alice-write".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        let app = vec!["wss://app".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);

        let plan = compiler.compile(&[timeline_interest(1, &["alice", "bob"])]).expect("compile");

        // Alice's write relay carries Alice via NIP-65 (and also AppRelay if Alice is there).
        let alice_plan = plan.per_relay.get("wss://alice-write").expect("alice-write");
        assert!(alice_plan.role_tags.contains(&RoutingSource::Nip65));

        // App relay carries both Alice and Bob via AppRelay lane.
        let app_plan = plan.per_relay.get("wss://app").expect("app");
        assert!(app_plan
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));

        // No one is unroutable.
        assert!(plan.unroutable_authors.is_empty());
    }

    /// Multi-author: one with NIP-65, one without; no app_relays. Only the
    /// known-mailbox author flies; the other lands in `unroutable_authors`.
    #[test]
    fn case_a_mixed_no_app_relays_isolates_unroutable() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pk("alice"),
            MailboxSnapshot {
                write_relays: vec!["wss://alice-write".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

        let plan = compiler.compile(&[timeline_interest(1, &["alice", "bob"])]).expect("compile");

        // Alice flies.
        assert!(plan.per_relay.contains_key("wss://alice-write"));

        // Bob is unroutable.
        assert!(plan.unroutable_authors.contains(&pk("bob")));
        assert!(!plan.unroutable_authors.contains(&pk("alice")));
    }

    /// NIP-65 known but `outbox_relays()` is empty AND no app_relays → the
    /// author is unroutable. (Empty mailbox is equivalent to missing one for
    /// the purpose of routing content; the kernel surfaces it identically.)
    #[test]
    fn case_a_empty_mailbox_without_app_relays_is_unroutable() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pk("alice"),
            MailboxSnapshot {
                write_relays: vec![],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

        let plan = compiler.compile(&[timeline_interest(1, &["alice"])]).expect("compile");

        assert!(plan.per_relay.is_empty());
        assert!(plan.unroutable_authors.contains(&pk("alice")));
    }

    // ── PD-033-C planner extension — indexer fallback arm (§4.3) ────────────
    //
    // The matrix below mirrors `kernel/discovery.rs::drain_unknown_oneshots`'s
    // profile-oneshot arm: kind:0/3/10002 + authors → `RelayRole::Indexer`.
    // Without this, deleting M1 in Stage 1 would mark every discovery-targeted
    // pubkey `unroutable` and the kernel would never fetch the profile.

    /// One-shot global profile fetch (the discovery-oneshot shape) with NO
    /// NIP-65 mailbox cached AND NO app_relays → routes to
    /// `bootstrap_indexer_relays` (lane `UserConfigured(Indexer)`). The author
    /// is NOT `unroutable`. This is the headline silent-loss regression the
    /// planner extension fixes.
    #[test]
    fn pd033c_case_a_oneshot_global_no_nip65_routes_to_bootstrap_indexer() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            /* indexer = */ &[],
            &[],
            &[],
            /* bootstrap_content = */ &[],
            &bootstrap_indexer,
        );

        // Profile-shape oneshot, scope Global — matches `oneshot.request(...)`.
        let interest = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: [pk("bob")].into_iter().collect(),
                kinds: [0u32, 3, 10002].into_iter().collect(),
                limit: Some(3),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
        };

        let plan = compiler.compile(&[interest]).expect("compile");
        let ix = plan
            .per_relay
            .get("wss://purplepag.es")
            .expect("bootstrap indexer must carry the discovery profile-oneshot");
        assert!(ix
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Indexer)));
        // Critical: Bob is NOT unroutable — the silent-loss invariant.
        assert!(
            plan.unroutable_authors.is_empty(),
            "PD-033-C invariant: discovery-oneshot authors with bootstrap-indexer \
             fallback must NOT be marked unroutable; got {:?}",
            plan.unroutable_authors
        );
    }

    /// Cold-start divergence regression: `lifecycle.indexer_relays` (the raw
    /// editable indexer rows) and `bootstrap_indexer_relays` (the kernel's
    /// `bootstrap_urls_for_role(RelayRole::Indexer)`, which carries
    /// `FALLBACK_INDEXER_RELAY` when no row is configured) are NOT
    /// interchangeable. M1's profile-oneshot arm rides the WITH-fallback form;
    /// the planner extension must do the same or cold-start sign-ins (no
    /// indexer row configured yet) silently lose discovery the moment Stage 1
    /// deletes M1. This test pins the divergence: raw indexer empty +
    /// bootstrap_indexer non-empty → discovery still lands.
    #[test]
    fn pd033c_case_a_cold_start_uses_bootstrap_indexer_not_raw_indexer() {
        let cache = InMemoryMailboxCache::new();
        // The cold-start case: NO operator-configured indexer rows. Raw
        // `indexer_relays` is empty (the kernel's `set_relay_edit_rows` filter
        // returned nothing); `bootstrap_indexer_relays` carries the fallback.
        let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            /* indexer (raw, no fallback) = */ &[],
            &[],
            &[],
            /* bootstrap_content = */ &[],
            &bootstrap_indexer,
        );

        let interest = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: [pk("bob")].into_iter().collect(),
                kinds: [0u32, 3, 10002].into_iter().collect(),
                limit: Some(3),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
        };

        let plan = compiler.compile(&[interest]).expect("compile");
        assert!(
            plan.per_relay.get("wss://purplepag.es").is_some(),
            "cold-start discovery MUST land on bootstrap_indexer even when raw \
             indexer_relays is empty (M1 parity)"
        );
        assert!(
            plan.unroutable_authors.is_empty(),
            "cold-start discovery author MUST NOT be unroutable"
        );
    }

    /// Counterpoint: a `Tailing` follow-feed interest (a non-discovery
    /// timeline) for the same NIP-65-unknown author MUST still be `unroutable`
    /// even when `bootstrap_indexer_relays` is set — the planner extension is
    /// strictly scoped to discovery oneshots; broader fallback would degrade
    /// routing for the 99% case (tailing follows ride NIP-65, indexer is
    /// discovery-only per T134).
    #[test]
    fn pd033c_case_a_tailing_no_nip65_remains_unroutable() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &[],
            &bootstrap_indexer,
        );

        // Plain timeline interest — Tailing lifecycle, exactly the shape that
        // must NOT be diverted to the indexer (would re-introduce the T134
        // anti-pattern of follow-feeds on purplepag.es).
        let plan = compiler
            .compile(&[timeline_interest(1, &["bob"])])
            .expect("compile");

        assert!(
            plan.per_relay.get("wss://purplepag.es").is_none(),
            "Tailing follow-feed must NOT route to bootstrap indexer (T134 invariant)"
        );
        assert!(
            plan.unroutable_authors.contains(&pk("bob")),
            "Tailing+Global without NIP-65/app-relays must remain unroutable"
        );
    }

    /// Counterpoint: a `OneShot + Account(x)` profile fetch is account-scoped
    /// (it ultimately resolves to a concrete account context). Today it stays
    /// `unroutable` rather than diverting to the indexer — gate is OneShot AND
    /// Global, not OneShot alone. This prevents account-scoped interests from
    /// being mistakenly placed on the cold-start indexer lane.
    #[test]
    fn pd033c_case_a_account_scoped_oneshot_does_not_indexer_fallback() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &[],
            &bootstrap_indexer,
        );

        let interest = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Account(pk("alice")),
            shape: InterestShape {
                authors: [pk("bob")].into_iter().collect(),
                kinds: [0u32, 3, 10002].into_iter().collect(),
                limit: Some(3),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
        };

        let plan = compiler.compile(&[interest]).expect("compile");
        assert!(
            plan.per_relay.get("wss://purplepag.es").is_none(),
            "Account-scoped OneShot must NOT divert to the bootstrap indexer lane"
        );
        assert!(plan.unroutable_authors.contains(&pk("bob")));
    }

    /// When `app_relays` ARE configured, the `if !landed` block never fires —
    /// the AppRelay lane already carried the author. The PD-033-C
    /// bootstrap-indexer arm must NOT additively route to the indexer in that
    /// case (would double-charge the indexer for a routable author).
    #[test]
    fn pd033c_case_a_oneshot_global_with_app_relays_skips_bootstrap_indexer() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
        let app = vec!["wss://user-app.example".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &app,
            &[],
            &bootstrap_indexer,
        );

        let interest = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: [pk("bob")].into_iter().collect(),
                kinds: [0u32, 3, 10002].into_iter().collect(),
                limit: Some(3),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
        };

        let plan = compiler.compile(&[interest]).expect("compile");
        // App relay carried Bob — indexer must be untouched.
        assert!(plan.per_relay.get("wss://user-app.example").is_some());
        assert!(
            plan.per_relay.get("wss://purplepag.es").is_none(),
            "PD-033-C bootstrap-indexer fallback must NOT fire when AppRelay \
             carried the author"
        );
        assert!(plan.unroutable_authors.is_empty());
    }

    /// Mixed multi-author: one author with NIP-65, one author without (and no
    /// app_relays). The NIP-65 author rides their write relay; the
    /// no-mailbox author falls back to the bootstrap indexer via the PD-033-C
    /// arm. Critically: neither lands in `unroutable_authors`.
    #[test]
    fn pd033c_case_a_mixed_authors_partial_nip65_landed_via_bootstrap_indexer() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(pk("alice"), MailboxSnapshot {
            write_relays: vec!["wss://alice-write".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        });
        let bootstrap_indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &[],
            &bootstrap_indexer,
        );

        let interest = LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: [pk("alice"), pk("bob")].into_iter().collect(),
                kinds: [0u32, 3, 10002].into_iter().collect(),
                limit: Some(3),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
        };

        let plan = compiler.compile(&[interest]).expect("compile");
        // Alice rides her NIP-65 write relay.
        assert!(plan.per_relay.get("wss://alice-write").is_some());
        // Bob lands on the bootstrap indexer via the PD-033-C arm.
        assert!(plan.per_relay.get("wss://purplepag.es").is_some());
        // Neither is unroutable.
        assert!(plan.unroutable_authors.is_empty());
    }
}
