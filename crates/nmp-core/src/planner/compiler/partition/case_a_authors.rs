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
//! - Author with NO NIP-65 mailbox AND no `app_relays` configured → the
//!   author is recorded in `unroutable` so the kernel can surface a UI
//!   diagnostic. The interest still flies to other authors' relays.
//!
//! The indexer set is NEVER consulted in this case. Indexers are discovery-
//! only (kind:0 / kind:3 / kind:10002 lookups driven by the kernel directly).
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestShape, LogicalInterest, NaddrCoord, Pubkey, RelayUrl},
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
pub(super) fn route(
    interest: &LogicalInterest,
    p_tag_values: &BTreeSet<Pubkey>,
    base_shape: &InterestShape,
    mailbox_cache: &dyn MailboxCache,
    app_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
    unroutable: &mut BTreeSet<Pubkey>,
) {
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
            unroutable.insert(author.clone());
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
}
