//! NIP-17 DM runtime state machine.
//!
//! Reconciles the active account and the host-supplied DM-inbox relay set
//! against the last-applied view, emitting the minimal set of side-effects a
//! host shell must drive:
//!
//! * push / withdraw the gift-wrap inbox interest for the active account so
//!   the kernel subscribes to kind:1059 envelopes addressed to that pubkey;
//! * fetch kind:10050 DM relay lists for the active account and visible DM
//!   peers so outbound sends can resolve self-copy and recipient inbox relays
//!   through the Rust-owned cache;
//! * publish a fresh kind:10050 DM relay-list event when the canonical relay
//!   set changes (so other clients can find the user as a DM recipient).
//!
//! This module is host-agnostic protocol orchestration — no I/O, no clocks,
//! no key access, no FFI. The host shell (a native app built on the
//! `nmp-uniffi` binding surface, or any other composition root) owns the
//! [`ActorCommand`](nmp_core::actor::ActorCommand) translation, the snapshot
//! projection wiring, and the lock that owns `DmRuntimeState` across ticks.
//! This crate just decides *what should happen* given the inputs.
//!
//! # Trellis-backed peer diff (#3116)
//!
//! The peer relay-list interest set's added/removed diff is NOT hand-rolled:
//! [`DmRuntimeState`] owns one session-persistent
//! [`nmp_core::trellis_reconciler::KeyedReconciler`]`<String, ()>` keyed by
//! peer pubkey (the last surviving hand-rolled family-shape reconciler the
//! #3115/#3116 sweep missed). [`DmRuntimeState::reconcile`] feeds it the
//! full desired peer set every call; Trellis returns an ORDERED
//! `Vec<ResourceCommand<()>>` — `Open` for a newly-desired peer, `Close` for
//! a peer no longer desired — which [`apply_peer_commands`] turns into
//! `PushPeerRelayListInterest` / `WithdrawPeerRelayListInterest` effects
//! **in Vec order** (never sorted). An empty desired map (account cleared or
//! switched) closes every currently-open peer in one call — the drain-on-close
//! semantics the pre-migration `mem::take` loop implemented.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::trellis_reconciler::KeyedReconciler;
use nmp_signer_iface::UnsignedEvent;
use trellis_core::{ResourceCommand, ResourceKey};

use crate::dm_relay_list::build_dm_relay_list_event;

/// Trellis-internal diagnostic scope label — never surfaced to a concept.
const PEER_RELAY_LIST_RECONCILER_SCOPE: &str = "nmp.nip17.dm-runtime.peer-relay-list.v1";

/// Reconciler state for a host-driven NIP-17 DM runtime.
///
/// Tracks the last pubkey the inbox interest was pushed for and the last
/// (account, relay-set) the kind:10050 was published for so [`reconcile`]
/// emits effects only on real change.
///
/// [`reconcile`]: DmRuntimeState::reconcile
pub struct DmRuntimeState {
    last_inbox_pubkey: Option<String>,
    last_published: Option<(String, BTreeSet<String>)>,
    /// Trellis-backed keyed reconciler over the live peer relay-list
    /// interest set (#3116) — see module docs. Owns the incremental
    /// desired-vs-live diff that used to be the hand-rolled
    /// `last_peer_relay_list_pubkeys: BTreeSet<String>` field.
    peer_reconciler: KeyedReconciler<String, ()>,
}

impl Default for DmRuntimeState {
    fn default() -> Self {
        Self {
            last_inbox_pubkey: None,
            last_published: None,
            peer_reconciler: KeyedReconciler::<String, ()>::new(
                PEER_RELAY_LIST_RECONCILER_SCOPE,
                peer_resource_key,
            )
            .expect("fresh KeyedReconciler construction over an empty graph cannot fail"), // doctrine-allow: D6 — construction over a brand-new empty graph before any transaction runs; `KeyedReconciler::new` can only fail on a Trellis-internal graph-build error, which is unreachable here (mirrors `nmp-core::kernel::kernel_new`'s identical precedent for `feed_author_reconciler`)
        }
    }
}

impl DmRuntimeState {
    /// Diff the new (`active_pubkey`, `read_relay_urls`) tuple against the last
    /// applied view and return the minimal list of effects the host must
    /// apply this tick.
    ///
    /// Effects, in order:
    /// 1. If the active account cleared, withdraw any standing inbox
    ///    interest and forget the last-published relay set.
    /// 2. If the active account changed, push a fresh inbox interest for
    ///    the new pubkey (and forget any prior account's last-published set
    ///    so the new account republishes its own).
    /// 3. If the canonical relay set (kind:10050 tags built by
    ///    [`build_dm_relay_list_event`]) differs from the last published
    ///    set for this account, emit a `PublishRelayList` carrying the
    ///    unsigned event the host should hand to the actor.
    ///
    /// An empty incoming relay set is a no-op for the publish path — we
    /// never emit a kind:10050 that would clear the user's cache. (The
    /// `nmp.nip17.publish_relay_list` action validator enforces the same
    /// guard on the dispatch seam.)
    #[must_use]
    pub fn reconcile(
        &mut self,
        active_pubkey: Option<&str>,
        read_relay_urls: &[String],
        peer_pubkeys: &[String],
    ) -> Vec<DmRuntimeEffect> {
        let mut effects = Vec::new();
        let active_pubkey = active_pubkey.filter(|pk| !pk.is_empty());
        let Some(account) = active_pubkey else {
            apply_peer_reconcile(&mut effects, &self.peer_reconciler, BTreeMap::new());
            if let Some(previous_account) = self.last_inbox_pubkey.take() {
                effects.push(DmRuntimeEffect::WithdrawOwnRelayListInterest(
                    previous_account,
                ));
                effects.push(DmRuntimeEffect::WithdrawInboxInterest);
            }
            self.last_published = None;
            return effects;
        };

        if self.last_inbox_pubkey.as_deref() != Some(account) {
            if let Some(previous_account) = self.last_inbox_pubkey.as_ref() {
                effects.push(DmRuntimeEffect::WithdrawOwnRelayListInterest(
                    previous_account.clone(),
                ));
            }
            apply_peer_reconcile(&mut effects, &self.peer_reconciler, BTreeMap::new());
            self.last_inbox_pubkey = Some(account.to_string());
            effects.push(DmRuntimeEffect::PushInboxInterest(account.to_string()));
            effects.push(DmRuntimeEffect::PushOwnRelayListInterest(
                account.to_string(),
            ));
        }

        if self
            .last_published
            .as_ref()
            .is_some_and(|(published_account, _)| published_account != account)
        {
            self.last_published = None;
        }

        let next_peers = peer_pubkeys
            .iter()
            .filter(|peer| !peer.is_empty() && peer.as_str() != account)
            .cloned()
            .collect::<BTreeSet<_>>();
        let peer_commands = self
            .peer_reconciler
            .reconcile(next_peers.into_iter().map(|peer| (peer, ())).collect());
        if let Ok(peer_commands) = peer_commands {
            apply_peer_commands(&mut effects, peer_commands);
        }

        let event = build_dm_relay_list_event(read_relay_urls);
        let relay_urls = relay_urls_from_event(&event);
        if relay_urls.is_empty() {
            return effects;
        }
        let relay_set = relay_urls.into_iter().collect::<BTreeSet<_>>();
        if self
            .last_published
            .as_ref()
            .is_some_and(|(published_account, published_set)| {
                published_account == account && published_set == &relay_set
            })
        {
            return effects;
        }

        self.last_published = Some((account.to_string(), relay_set.clone()));
        effects.push(DmRuntimeEffect::PublishRelayList { event, relay_set });
        effects
    }
}

/// A side-effect the host shell must apply after calling
/// [`DmRuntimeState::reconcile`].
///
/// Each variant maps to one `ActorCommand` on the host side, but this enum
/// stays `ActorCommand`-free so the protocol crate doesn't take a structural
/// dependency on the actor wire shape (the host owns that translation).
#[derive(Debug, PartialEq, Eq)]
pub enum DmRuntimeEffect {
    /// Subscribe the kernel to gift-wrap (kind:1059) envelopes addressed to
    /// this pubkey. The host translates to `InterestsCommand::EnsureInterest` with
    /// `active_giftwrap_inbox_interest(&pubkey)`.
    PushInboxInterest(String),
    /// Drop the standing gift-wrap inbox interest (account logged out or
    /// switched). The host translates to `InterestsCommand::DropInterestOwner`
    /// with `active_giftwrap_inbox_interest_id()`.
    WithdrawInboxInterest,
    /// Fetch the active account's own kind:10050 DM relay list. Outbound
    /// NIP-17 sends need this for the self-copy envelope.
    PushOwnRelayListInterest(String),
    /// Drop the active account's own kind:10050 relay-list interest when the
    /// account logs out or switches.
    WithdrawOwnRelayListInterest(String),
    /// Fetch this peer's kind:10050 DM relay list. The host translates to
    /// `InterestsCommand::EnsureInterest` with
    /// `peer_dm_relay_list_interest(&pubkey)`.
    PushPeerRelayListInterest(String),
    /// Drop a peer kind:10050 relay-list interest when the conversation
    /// disappears or the active account changes.
    WithdrawPeerRelayListInterest(String),
    /// Publish the user's own kind:10050 DM relay-list. `event` is the
    /// unsigned event built by [`build_dm_relay_list_event`] (D7 sentinel
    /// `created_at: 0`, empty pubkey — the actor stamps and signs).
    /// `relay_set` is the canonical set the reconciler recorded as
    /// last-published so a no-op tick is detected next round.
    PublishRelayList {
        event: UnsignedEvent,
        relay_set: BTreeSet<String>,
    },
}

fn relay_urls_from_event(event: &UnsignedEvent) -> Vec<String> {
    event
        .tags
        .iter()
        .filter_map(|tag| match tag.as_slice() {
            [marker, url] if marker == "relay" => Some(url.clone()),
            _ => None,
        })
        .collect()
}

/// Derives a peer pubkey's single-segment `ResourceKey`. Single-segment
/// means [`ResourceKey::as_str`] recovers the ORIGINAL peer pubkey unchanged,
/// so [`apply_peer_commands`] never needs a separate translation table.
fn peer_resource_key(peer: &String) -> ResourceKey {
    ResourceKey::new(peer.clone())
}

fn apply_peer_reconcile(
    effects: &mut Vec<DmRuntimeEffect>,
    reconciler: &KeyedReconciler<String, ()>,
    desired: BTreeMap<String, ()>,
) {
    if let Ok(commands) = reconciler.reconcile(desired) {
        apply_peer_commands(effects, commands);
    }
}

/// Applies a Trellis resource plan **in `Vec` order** — never sort or
/// parallelize; LIFO close-vs-close correctness on scope teardown lives in
/// this order (#3116 VERIFY-FIRST note). `Open` maps to
/// [`DmRuntimeEffect::PushPeerRelayListInterest`], `Close` to
/// [`DmRuntimeEffect::WithdrawPeerRelayListInterest`].
fn apply_peer_commands(effects: &mut Vec<DmRuntimeEffect>, commands: Vec<ResourceCommand<()>>) {
    for command in commands {
        match command {
            ResourceCommand::Open { key, .. } => {
                effects.push(DmRuntimeEffect::PushPeerRelayListInterest(
                    key.as_str().to_string(),
                ));
            }
            ResourceCommand::Close { key, .. } => {
                effects.push(DmRuntimeEffect::WithdrawPeerRelayListInterest(
                    key.as_str().to_string(),
                ));
            }
            ResourceCommand::Replace { .. } | ResourceCommand::Refresh { .. } => {
                // Never emitted: the payload is `()`, so a same-key join can
                // never carry a changed payload (`KeyedReconciler::new`'s
                // planner only opens added / closes removed) — exhaustive
                // match, not a reachable branch.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relays(urls: &[&str]) -> Vec<String> {
        urls.iter().map(|url| url.to_string()).collect()
    }

    #[test]
    fn active_account_pushes_interest_and_publishes_once() {
        let mut state = DmRuntimeState::default();
        let effects = state.reconcile(Some("alice"), &relays(&["wss://a.example"]), &[]);
        assert!(matches!(
            effects.as_slice(),
            [
                DmRuntimeEffect::PushInboxInterest(pk),
                DmRuntimeEffect::PushOwnRelayListInterest(own_pk),
                DmRuntimeEffect::PublishRelayList { relay_set, .. }
            ] if pk == "alice" && own_pk == "alice" && relay_set.contains("wss://a.example")
        ));
        assert!(state
            .reconcile(Some("alice"), &relays(&["wss://a.example"]), &[])
            .is_empty());
    }

    #[test]
    fn relay_set_changes_republish_without_repush_interest() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]), &[]);
        let effects = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example", "wss://b.example"]),
            &[],
        );
        assert!(matches!(
            effects.as_slice(),
            [DmRuntimeEffect::PublishRelayList { relay_set, .. }]
                if relay_set.contains("wss://a.example")
                    && relay_set.contains("wss://b.example")
        ));
    }

    #[test]
    fn account_switch_replaces_interest_and_republishes_same_relays() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]), &[]);
        let effects = state.reconcile(Some("bob"), &relays(&["wss://a.example"]), &[]);
        assert!(matches!(
            effects.as_slice(),
            [
                DmRuntimeEffect::WithdrawOwnRelayListInterest(previous_pk),
                DmRuntimeEffect::PushInboxInterest(pk),
                DmRuntimeEffect::PushOwnRelayListInterest(own_pk),
                DmRuntimeEffect::PublishRelayList { relay_set, .. }
            ] if previous_pk == "alice"
                && pk == "bob"
                && own_pk == "bob"
                && relay_set.contains("wss://a.example")
        ));
    }

    #[test]
    fn logout_withdraws_active_interest_slot() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]), &[]);
        assert_eq!(
            state.reconcile(None, &relays(&["wss://a.example"]), &[]),
            vec![
                DmRuntimeEffect::WithdrawOwnRelayListInterest("alice".to_string()),
                DmRuntimeEffect::WithdrawInboxInterest,
            ]
        );
    }

    #[test]
    fn visible_peer_pushes_kind10050_interest_once() {
        let mut state = DmRuntimeState::default();
        let effects = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example"]),
            &["bob".to_string()],
        );
        assert!(
            effects.contains(&DmRuntimeEffect::PushPeerRelayListInterest(
                "bob".to_string()
            ))
        );
        let effects = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example"]),
            &["bob".to_string()],
        );
        assert!(
            !effects.contains(&DmRuntimeEffect::PushPeerRelayListInterest(
                "bob".to_string()
            )),
            "peer relay-list interest must be idempotent"
        );
    }

    #[test]
    fn peer_interest_is_withdrawn_on_account_switch() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example"]),
            &["bob".to_string()],
        );
        let effects = state.reconcile(Some("carol"), &relays(&["wss://a.example"]), &[]);
        assert!(
            effects.contains(&DmRuntimeEffect::WithdrawPeerRelayListInterest(
                "bob".to_string()
            ))
        );
    }

    // #3116 equivalence: `peer_reconciler`'s Trellis `full_recompute_matches`
    // oracle (the leak-audit guarantee #3115/#3116 wires into every migrated
    // reconciler) plus a grow/shrink/drain-on-close parity pass mirroring
    // `feed_author_refs_tests_equivalence.rs`.

    #[test]
    fn peer_reconciler_full_recompute_oracle_across_grow_shrink_and_drain_on_close() {
        let mut state = DmRuntimeState::default();

        // Grow: alice opens with peer bob.
        let effects = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example"]),
            &["bob".to_string()],
        );
        assert!(
            effects.contains(&DmRuntimeEffect::PushPeerRelayListInterest(
                "bob".to_string()
            ))
        );
        assert!(state.peer_reconciler.full_recompute_matches());

        // Grow further: bob stays, carol joins — only carol pushes.
        let effects = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example"]),
            &["bob".to_string(), "carol".to_string()],
        );
        assert_eq!(
            effects,
            vec![DmRuntimeEffect::PushPeerRelayListInterest(
                "carol".to_string()
            )],
            "only the newly-desired peer pushes; bob is untouched"
        );
        assert!(state.peer_reconciler.full_recompute_matches());

        // Shrink: bob drops, carol stays — only bob withdraws.
        let effects = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example"]),
            &["carol".to_string()],
        );
        assert_eq!(
            effects,
            vec![DmRuntimeEffect::WithdrawPeerRelayListInterest(
                "bob".to_string()
            )],
            "only the dropped peer withdraws; carol is untouched"
        );
        assert!(state.peer_reconciler.full_recompute_matches());

        // Drain-on-close: account clears — carol (the last live peer)
        // withdraws alongside the own-relay-list and inbox interests.
        let effects = state.reconcile(None, &relays(&["wss://a.example"]), &[]);
        assert_eq!(
            effects,
            vec![
                DmRuntimeEffect::WithdrawPeerRelayListInterest("carol".to_string()),
                DmRuntimeEffect::WithdrawOwnRelayListInterest("alice".to_string()),
                DmRuntimeEffect::WithdrawInboxInterest,
            ]
        );
        assert!(state.peer_reconciler.full_recompute_matches());

        // A second clear is a no-op — nothing left to drain.
        assert!(state
            .reconcile(None, &relays(&["wss://a.example"]), &[])
            .is_empty());
        assert!(state.peer_reconciler.full_recompute_matches());
    }

    #[test]
    fn account_switch_drains_all_peers_via_reconciler_even_when_pubkey_recurs() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example"]),
            &["bob".to_string()],
        );
        assert!(state.peer_reconciler.full_recompute_matches());

        // bob is desired again under the NEW account — the pre-migration
        // hand-rolled diff unconditionally withdrew every prior-account peer
        // on switch, then re-pushed the new account's desired set from
        // scratch; the migrated reconciler preserves this via an explicit
        // close-all before the fresh diff, so bob withdraws AND re-pushes
        // even though the pubkey recurs.
        let effects = state.reconcile(
            Some("carol"),
            &relays(&["wss://a.example"]),
            &["bob".to_string()],
        );
        assert!(
            effects.contains(&DmRuntimeEffect::WithdrawPeerRelayListInterest(
                "bob".to_string()
            ))
        );
        assert!(
            effects.contains(&DmRuntimeEffect::PushPeerRelayListInterest(
                "bob".to_string()
            )),
            "bob re-pushes fresh under the new account even though the pubkey recurs"
        );
        assert!(state.peer_reconciler.full_recompute_matches());
    }
}
