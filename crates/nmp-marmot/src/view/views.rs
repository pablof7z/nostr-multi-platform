//! `GroupList`, `GroupMessages` reactive views per
//! `docs/plan/marmot-mls.md` §Step 1 + mdk-api.md §6.
//!
//! `GroupMessages` is relay-pinned to the group relay (kind:445) via
//! `InterestShape::relay_pin` (ADR-0012). `GroupList` projects
//! off service-materialised state (the actual member set + decrypted history
//! come from MDK, not the raw wire), so its dependency surface is the
//! relay-pinned kind:445 stream that triggers re-projection ticks; the
//! decrypted payload is filled by the service/actor layer (same scope-cut
//! as nmp-nip29's Step 0 views).

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
use serde::{Deserialize, Serialize};

use super::shared::{EventAccumulator, EventAccumulatorDelta};
use crate::interest::{KIND_GROUP_MESSAGE, KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY};

// ─── GroupList ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GroupListSpec {
    /// The local identity pubkey (hex) whose joined groups to list.
    pub self_pubkey: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupListEntry {
    pub group_id_hex: String,
    pub group_relay_url: String,
    pub name: String,
    pub unread: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupListPayload {
    pub groups: Vec<GroupListEntry>,
}

/// List of joined + pending Marmot groups with unread counts. The authoritative
/// group set comes from `MDK::get_groups()` via [`crate::service`]; this view's
/// snapshot is filled by the service/actor layer (the wire is ciphertext).
pub struct GroupListView;
impl GroupListView {
    pub const NAMESPACE: &'static str = "nmp.marmot.group_list";

    #[must_use] 
    pub fn key(spec: &GroupListSpec) -> String {
        spec.self_pubkey.clone()
    }
    #[must_use] 
    pub fn dependencies(_spec: &GroupListSpec) -> ViewDependencies {
        // KeyPackage stream (own publications, standard outbox — no pin) is the
        // structural trigger surface; group membership itself is MDK state.
        ViewDependencies {
            kinds: vec![KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY],
            ..Default::default()
        }
    }
    #[must_use] 
    pub fn open(_c: &ViewContext, _spec: GroupListSpec) -> (EventAccumulator, GroupListPayload) {
        (
            EventAccumulator::default(),
            GroupListPayload { groups: Vec::new() },
        )
    }
    #[must_use]
    pub fn on_event_inserted(
        _c: &ViewContext,
        s: &mut EventAccumulator,
        e: &KernelEvent,
    ) -> Option<EventAccumulatorDelta> {
        s.insert(e)
    }
    #[must_use]
    pub fn on_event_removed(
        _c: &ViewContext,
        s: &mut EventAccumulator,
        id: &EventId,
    ) -> Option<EventAccumulatorDelta> {
        s.remove(id)
    }
    #[must_use]
    pub fn on_event_replaced(
        _c: &ViewContext,
        s: &mut EventAccumulator,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<EventAccumulatorDelta> {
        s.replace(old, e)
    }
    #[must_use]
    pub fn snapshot(_c: &ViewContext, _state: &EventAccumulator) -> GroupListPayload {
        // Authoritative list is MDK-side; the service/actor layer fills this
        // snapshot. The structural accumulator only drives re-projection ticks.
        GroupListPayload { groups: Vec::new() }
    }
}

// ─── GroupMessages ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GroupMessagesSpec {
    /// Hex MLS group id.
    pub group_id_hex: String,
    /// The group relay all kind:445 events are pinned to.
    pub group_relay_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupMessageEntry {
    pub message_id: String,
    pub sender_pubkey: String,
    pub created_at: u64,
    pub content: String,
    pub epoch: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GroupMessagesPayload {
    pub messages: Vec<GroupMessageEntry>,
}

/// Paginated decrypted message stream for a group; live-updates on new epoch.
/// Relay-pinned to the group relay (kind:445). Decrypted content is filled by
/// the service after `MDK::process_message`.
pub struct GroupMessagesView;
impl GroupMessagesView {
    pub const NAMESPACE: &'static str = "nmp.marmot.group_messages";

    #[must_use] 
    pub fn key(spec: &GroupMessagesSpec) -> String {
        spec.group_id_hex.clone()
    }
    #[must_use] 
    pub fn dependencies(spec: &GroupMessagesSpec) -> ViewDependencies {
        // kind:445 group-event stream, pinned to the group relay (ADR-0012
        // third lane). The structural surface declares the kind; `relay_pin`
        // declares the host affinity in the data model.
        ViewDependencies {
            kinds: vec![KIND_GROUP_MESSAGE],
            relay_pin: Some(spec.group_relay_url.clone()),
            ..Default::default()
        }
    }
    #[must_use] 
    pub fn open(
        _c: &ViewContext,
        _spec: GroupMessagesSpec,
    ) -> (EventAccumulator, GroupMessagesPayload) {
        (
            EventAccumulator::default(),
            GroupMessagesPayload {
                messages: Vec::new(),
            },
        )
    }
    #[must_use]
    pub fn on_event_inserted(
        _c: &ViewContext,
        s: &mut EventAccumulator,
        e: &KernelEvent,
    ) -> Option<EventAccumulatorDelta> {
        s.insert(e)
    }
    #[must_use]
    pub fn on_event_removed(
        _c: &ViewContext,
        s: &mut EventAccumulator,
        id: &EventId,
    ) -> Option<EventAccumulatorDelta> {
        s.remove(id)
    }
    #[must_use]
    pub fn on_event_replaced(
        _c: &ViewContext,
        s: &mut EventAccumulator,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<EventAccumulatorDelta> {
        s.replace(old, e)
    }
    #[must_use]
    pub fn snapshot(_c: &ViewContext, _state: &EventAccumulator) -> GroupMessagesPayload {
        // Decrypted messages are filled by the service after MDK processing;
        // the structural accumulator only drives re-projection ticks.
        GroupMessagesPayload {
            messages: Vec::new(),
        }
    }
}

// ─── KeyPackageLookup ────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct KeyPackageLookupSpec {
    /// Nostr pubkey (hex) to fetch KeyPackages for.
    pub owner_pubkey: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct KeyPackageLookupPayload {
    pub owner_pubkey: String,
    /// `true` once at least one kind:30443/443 event has arrived for this author.
    pub found: bool,
}

/// Declares the kind:30443/443 relay-subscription shape for fetching a peer's
/// KeyPackage from their NIP-65 write relays.
///
/// ORPHANED (2026-05-31): this view was meant to be opened via
/// `OpenView { namespace: "marmot.key_package_lookup", key: pubkey }`, but the
/// `KernelAction::OpenView` reducer arm is an unwired stub that opens no
/// subscription (it silently echoes `ViewOpened`). The key-package fetch now
/// runs directly via `interest::key_package_lookup_interest` +
/// `app.push_interest(...)` (the same pattern as the welcome/group-message
/// legs), so this type has no live caller. It is retained as the canonical
/// declaration of the lookup interest shape pending V-110 (wire `OpenView` to
/// compile a view's dependencies, or remove the unused View machinery).
///
/// Signed-event caching is handled by the app's `RawEventObserver` tap (which
/// receives the full signed event including `sig`) calling
/// `MarmotService::cache_key_package`.
pub struct KeyPackageLookupView;

impl KeyPackageLookupView {
    pub const NAMESPACE: &'static str = "nmp.marmot.key_package_lookup";

    #[must_use] 
    pub fn key(spec: &KeyPackageLookupSpec) -> String {
        spec.owner_pubkey.clone()
    }
    #[must_use] 
    pub fn dependencies(spec: &KeyPackageLookupSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY],
            authors: vec![spec.owner_pubkey.clone()],
            ..Default::default()
        }
    }
    #[must_use] 
    pub fn open(
        _c: &ViewContext,
        spec: KeyPackageLookupSpec,
    ) -> (EventAccumulator, KeyPackageLookupPayload) {
        (
            EventAccumulator::default(),
            KeyPackageLookupPayload { owner_pubkey: spec.owner_pubkey, found: false },
        )
    }
    #[must_use]
    pub fn on_event_inserted(_c: &ViewContext, s: &mut EventAccumulator, e: &KernelEvent) -> Option<EventAccumulatorDelta> {
        s.insert(e)
    }
    #[must_use]
    pub fn on_event_removed(_c: &ViewContext, s: &mut EventAccumulator, id: &EventId) -> Option<EventAccumulatorDelta> {
        s.remove(id)
    }
    #[must_use]
    pub fn on_event_replaced(_c: &ViewContext, s: &mut EventAccumulator, old: &EventId, e: &KernelEvent) -> Option<EventAccumulatorDelta> {
        s.replace(old, e)
    }
    #[must_use] 
    pub fn snapshot(_c: &ViewContext, state: &EventAccumulator) -> KeyPackageLookupPayload {
        KeyPackageLookupPayload {
            owner_pubkey: String::new(),
            found: !state.events.is_empty(),
        }
    }
}
