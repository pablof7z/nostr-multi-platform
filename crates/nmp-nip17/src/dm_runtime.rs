//! NIP-17 DM runtime state machine.
//!
//! Reconciles the active account and the host-supplied DM-inbox relay set
//! against the last-applied view, emitting the minimal set of side-effects a
//! host shell must drive:
//!
//! * push / withdraw the gift-wrap inbox interest for the active account so
//!   the kernel subscribes to kind:1059 envelopes addressed to that pubkey;
//! * publish a fresh kind:10050 DM relay-list event when the canonical relay
//!   set changes (so other clients can find the user as a DM recipient).
//!
//! This module is host-agnostic protocol orchestration — no I/O, no clocks,
//! no key access, no FFI. The host shell (e.g. `apps/chirp`) owns the
//! [`ActorCommand`](nmp_core::ActorCommand) translation, the snapshot
//! projection wiring, and the lock that owns `DmRuntimeState` across ticks.
//! This crate just decides *what should happen* given the inputs.

use std::collections::BTreeSet;

use nmp_core::substrate::UnsignedEvent;

use crate::dm_relay_list::build_dm_relay_list_event;

/// Reconciler state for a host-driven NIP-17 DM runtime.
///
/// Tracks the last pubkey the inbox interest was pushed for and the last
/// (account, relay-set) the kind:10050 was published for so [`reconcile`]
/// emits effects only on real change.
///
/// [`reconcile`]: DmRuntimeState::reconcile
#[derive(Default)]
pub struct DmRuntimeState {
    last_inbox_pubkey: Option<String>,
    last_published: Option<(String, BTreeSet<String>)>,
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
    ) -> Vec<DmRuntimeEffect> {
        let mut effects = Vec::new();
        let active_pubkey = active_pubkey.filter(|pk| !pk.is_empty());
        let Some(account) = active_pubkey else {
            if self.last_inbox_pubkey.take().is_some() {
                effects.push(DmRuntimeEffect::WithdrawInboxInterest);
            }
            self.last_published = None;
            return effects;
        };

        if self.last_inbox_pubkey.as_deref() != Some(account) {
            self.last_inbox_pubkey = Some(account.to_string());
            effects.push(DmRuntimeEffect::PushInboxInterest(account.to_string()));
        }

        if self
            .last_published
            .as_ref()
            .is_some_and(|(published_account, _)| published_account != account)
        {
            self.last_published = None;
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
    /// this pubkey. The host translates to `ActorCommand::PushInterest` with
    /// `active_giftwrap_inbox_interest(&pubkey)`.
    PushInboxInterest(String),
    /// Drop the standing gift-wrap inbox interest (account logged out or
    /// switched). The host translates to `ActorCommand::WithdrawInterest`
    /// with `active_giftwrap_inbox_interest_id()`.
    WithdrawInboxInterest,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn relays(urls: &[&str]) -> Vec<String> {
        urls.iter().map(|url| url.to_string()).collect()
    }

    #[test]
    fn active_account_pushes_interest_and_publishes_once() {
        let mut state = DmRuntimeState::default();
        let effects = state.reconcile(Some("alice"), &relays(&["wss://a.example"]));
        assert!(matches!(
            effects.as_slice(),
            [
                DmRuntimeEffect::PushInboxInterest(pk),
                DmRuntimeEffect::PublishRelayList { relay_set, .. }
            ] if pk == "alice" && relay_set.contains("wss://a.example")
        ));
        assert!(state
            .reconcile(Some("alice"), &relays(&["wss://a.example"]))
            .is_empty());
    }

    #[test]
    fn relay_set_changes_republish_without_repush_interest() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]));
        let effects = state.reconcile(
            Some("alice"),
            &relays(&["wss://a.example", "wss://b.example"]),
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
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]));
        let effects = state.reconcile(Some("bob"), &relays(&["wss://a.example"]));
        assert!(matches!(
            effects.as_slice(),
            [
                DmRuntimeEffect::PushInboxInterest(pk),
                DmRuntimeEffect::PublishRelayList { relay_set, .. }
            ] if pk == "bob" && relay_set.contains("wss://a.example")
        ));
    }

    #[test]
    fn logout_withdraws_active_interest_slot() {
        let mut state = DmRuntimeState::default();
        let _ = state.reconcile(Some("alice"), &relays(&["wss://a.example"]));
        assert_eq!(
            state.reconcile(None, &relays(&["wss://a.example"])),
            vec![DmRuntimeEffect::WithdrawInboxInterest]
        );
    }
}
