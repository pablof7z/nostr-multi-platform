//! `PublishPlan` and `RelayPin` — the typed carrier the publish planner
//! consults for routing.
//!
//! Per `routing.md` §5: every NIP-29 action emits a `PublishPlan` with a
//! `Some(RelayPin)` set by the action that knows the host. The planner
//! consults this typed field for routing; it does NOT inspect event tags to
//! *derive* routing. The planner's only tag inspection is a defensive refusal
//! when an event carries `["h", _]` and `pin_to: None` (the
//! `MissingHostPinForGroupEvent` error per routing.md §5).
//!
//! This module is the typed carrier; the planner integration lands when M2's
//! publish planner is implemented (M6 + this milestone) — for M11.5 Step 0
//! the carrier is the contract the actions agree to.

use serde::{Deserialize, Serialize};

use crate::group_id::{GroupId, RelayUrl};

/// Routing pin: a single relay URL the publish must target exclusively.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct RelayPin {
    pub relay_url: RelayUrl,
    /// Carries the typed `GroupId` for diagnostics + audit; the planner uses
    /// `relay_url` for routing and ignores this.
    pub source_group: Option<GroupId>,
}

impl RelayPin {
    pub fn for_group(group: &GroupId) -> Self {
        Self {
            relay_url: group.host_relay_url.clone(),
            source_group: Some(group.clone()),
        }
    }
}

/// Pre-signing publish plan: the unsigned event shape + the typed routing
/// pin. The actor's signer-bridge converts this to a signed event and the
/// planner dispatches via the pin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublishPlan {
    pub kind: u32,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    /// Host-relay-pin per routing.md §5. Always `Some(_)` for NIP-29 actions.
    pub pin_to: Option<RelayPin>,
}

impl PublishPlan {
    pub fn pinned(group: &GroupId, kind: u32, content: impl Into<String>, tags: Vec<Vec<String>>) -> Self {
        Self {
            kind,
            content: content.into(),
            tags,
            pin_to: Some(RelayPin::for_group(group)),
        }
    }

    /// Defensive structural check the planner performs at construction time
    /// per routing.md §5: any event carrying `["h", _]` MUST have
    /// `pin_to.is_some()`, otherwise reject with `MissingHostPinForGroupEvent`.
    /// This is the privacy-leak prevention guard.
    pub fn validate_no_unpinned_h(&self) -> Result<(), PublishPlanError> {
        let has_h = self.tags.iter().any(|t| t.len() >= 2 && t[0] == "h");
        if has_h && self.pin_to.is_none() {
            return Err(PublishPlanError::MissingHostPinForGroupEvent);
        }
        Ok(())
    }

    /// Consume the plan into the [`ActorCommand`] that publishes its event,
    /// host-pinned to the plan's relay.
    ///
    /// This is the bridge that closes the long-standing gap between the typed
    /// `ActionModule` validators (which built a `PublishPlan` and then *discarded*
    /// it, returning `Ok(())`) and the actor: instead of the executor in
    /// `ffi.rs` hand-rebuilding the `UnsignedEvent` (and duplicating every
    /// action's tag logic), the action's own `build_plan()` is the single
    /// source of truth and this method turns it into a dispatchable command.
    ///
    /// The built `UnsignedEvent` carries an empty `pubkey` placeholder — the
    /// actor derives it from the active identity at sign time and overwrites
    /// the field (see `ActorCommand::PublishUnsignedEventToRelays`).
    /// `created_at` is left as `0`: D7 — this crate runs as an `ActionModule`
    /// executor with no kernel handle, so it cannot read the wall clock. The
    /// `0` is a "stamp me" sentinel; the actor's `PublishUnsignedEventToRelays`
    /// dispatch arm fills it in from `kernel.now_secs()`.
    ///
    /// Routes via [`ActorCommand::PublishUnsignedEventToRelays`] pinned to
    /// exactly the plan's host relay — a NIP-29 group event must reach the
    /// group's own host relay, never the author's NIP-65 outbox. Returns `Err`
    /// when `pin_to` is `None`; every NIP-29 action builds its plan with
    /// [`PublishPlan::pinned`] (always `Some(_)`), so this is a defensive
    /// guard the current callers never trip.
    pub fn into_actor_command(self) -> Result<nmp_core::ActorCommand, String> {
        use nmp_core::substrate::UnsignedEvent;
        use nmp_core::ActorCommand;
        let relay = self
            .pin_to
            .ok_or_else(|| "publish plan has no relay pin".to_string())?
            .relay_url;
        Ok(ActorCommand::PublishUnsignedEventToRelays {
            event: UnsignedEvent {
                pubkey: String::new(),
                kind: self.kind,
                tags: self.tags,
                content: self.content,
                // D7: `0` sentinel — the actor stamps the real clock value
                // (see the `PublishUnsignedEventToRelays` dispatch arm).
                created_at: 0,
            },
            relays: vec![relay],
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PublishPlanError {
    MissingHostPinForGroupEvent,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> GroupId { GroupId::new("wss://h.example.com", "room") }

    #[test]
    fn pinned_carries_host_relay() {
        let p = PublishPlan::pinned(&g(), 9, "hi", vec![vec!["h".into(), "room".into()]]);
        let pin = p.pin_to.unwrap();
        assert_eq!(pin.relay_url, "wss://h.example.com");
        assert_eq!(pin.source_group.unwrap(), g());
    }

    #[test]
    fn validate_rejects_unpinned_h_tag() {
        let mut p = PublishPlan::pinned(&g(), 9, "hi", vec![vec!["h".into(), "room".into()]]);
        p.pin_to = None;
        assert_eq!(
            p.validate_no_unpinned_h().unwrap_err(),
            PublishPlanError::MissingHostPinForGroupEvent
        );
    }

    #[test]
    fn validate_passes_pinned_h_tag() {
        let p = PublishPlan::pinned(&g(), 9, "hi", vec![vec!["h".into(), "room".into()]]);
        assert!(p.validate_no_unpinned_h().is_ok());
    }

    #[test]
    fn validate_passes_no_h_tag() {
        let p = PublishPlan {
            kind: 1,
            content: "public".into(),
            tags: vec![],
            pin_to: None,
        };
        assert!(p.validate_no_unpinned_h().is_ok());
    }

    #[test]
    fn into_actor_command_publishes_host_pinned_unsigned_event() {
        use nmp_core::ActorCommand;
        let p = PublishPlan::pinned(&g(), 9, "hi", vec![vec!["h".into(), "room".into()]]);
        match p.into_actor_command().expect("pinned plan converts") {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                // Pinned to EXACTLY the group's host relay — never the
                // author's NIP-65 outbox.
                assert_eq!(relays, vec!["wss://h.example.com".to_string()]);
                assert_eq!(event.kind, 9);
                assert_eq!(event.content, "hi");
                assert_eq!(event.tags, vec![vec!["h".to_string(), "room".to_string()]]);
                // `pubkey` is a placeholder — the actor fills it at sign time.
                assert!(event.pubkey.is_empty());
                // D7: `created_at` is the `0` sentinel — the actor stamps the
                // real clock value; this crate has no kernel handle.
                assert_eq!(event.created_at, 0);
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn into_actor_command_rejects_unpinned_plan() {
        // Defensive guard: every NIP-29 action builds its plan with
        // `PublishPlan::pinned` (always `Some(_)`), so this branch is
        // unreachable from real callers — but the conversion must still fail
        // closed rather than route a group event through the NIP-65 outbox.
        let p = PublishPlan {
            kind: 1,
            content: "x".into(),
            tags: vec![],
            pin_to: None,
        };
        assert!(p.into_actor_command().is_err());
    }
}
