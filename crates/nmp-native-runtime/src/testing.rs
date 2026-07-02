//! Native Rust test-support helpers.
//!
//! These are not C ABI symbols. They let Rust integration tests drive the
//! native runtime directly; UniFFI-side tests reach the same methods through
//! the generated `nmp-uniffi` bindings (there is no separate `nmp-ffi` C/JNI
//! test-support wrapper crate).

#![cfg(any(test, feature = "test-support"))]

use std::sync::atomic::Ordering;
use std::time::Duration;

use nmp_core::actor::{ActorCommand, TestSupportCommand};
use zeroize::Zeroizing;

use crate::NmpApp;

impl NmpApp {
    /// Return the actor command channel's approximate queue depth.
    #[must_use]
    pub fn queue_depth_for_test(&self) -> u64 {
        self.queue_depth.load(Ordering::Relaxed)
    }

    /// Return the actor command lane's cumulative shed-load drops.
    #[must_use]
    pub fn command_drops_for_test(&self) -> u64 {
        self.tx.command_drops()
    }

    /// Set the actor command channel's approximate queue depth.
    pub fn set_queue_depth_for_test(&self, depth: u64) {
        self.queue_depth.store(depth, Ordering::Relaxed);
    }

    /// Return the monotone count of commands sent through the runtime handle.
    #[must_use]
    pub fn send_cmd_count_for_test(&self) -> u64 {
        self.send_cmd_count.load(Ordering::Relaxed)
    }

    /// Return the most recent command variant tag recorded by the send seam.
    #[must_use]
    pub fn last_cmd_tag_for_test(&self) -> Option<&'static str> {
        self.last_cmd_tag.lock().ok().and_then(|tag| *tag)
    }

    /// Sign in with a local nsec through the actor-owned identity reducer.
    pub fn signin_nsec_for_test(&self, secret: impl Into<String>, make_active: bool) {
        self.add_signer(
            nmp_core::SignerSource::LocalNsec(Zeroizing::new(secret.into())),
            make_active,
        );
    }

    /// Inject one real signed NIP-01 JSON event through the production
    /// verification path, then enqueue it on the actor test-support ingest lane.
    #[must_use]
    pub fn inject_signed_event_json_for_test(&self, event_json: &str) -> bool {
        use nostr::JsonUtil;

        let nostr_event = match nostr::Event::from_json(event_json) {
            Ok(event) => event,
            Err(_) => return false,
        };
        let raw = nmp_store::RawEvent {
            id: nostr_event.id.to_hex(),
            pubkey: nostr_event.pubkey.to_hex(),
            created_at: nostr_event.created_at.as_secs(),
            kind: nostr_event.kind.as_u16() as u32,
            tags: nostr_event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect(),
            content: nostr_event.content.clone(),
            sig: nostr_event.sig.to_string(),
        };
        let Ok(verified) = nmp_store::VerifiedEvent::try_from_raw(raw) else {
            return false;
        };
        self.send_cmd(ActorCommand::TestSupport(
            TestSupportCommand::IngestPreVerifiedEvents(vec![verified]),
        ));
        true
    }

    /// Block until the actor dispatches every command enqueued before this call.
    #[must_use]
    pub fn wait_barrier_for_test(&self, timeout: Duration) -> bool {
        nmp_core::testing::wait_barrier(&self.actor_sender(), timeout)
    }
}

#[cfg(test)]
mod tests {
    use nmp_core::actor::{ActorCommand, LifecycleCommand};

    #[test]
    fn send_cmd_sheds_when_actor_inbox_is_full() {
        let app = crate::new_app();
        let capacity = nmp_core::actor::ACTOR_INBOX_CAPACITY as u64;

        for _ in 0..capacity {
            app.send_cmd(ActorCommand::Lifecycle(
                LifecycleCommand::MarkChangedSinceEmit,
            ));
        }
        assert_eq!(app.queue_depth_for_test(), capacity);
        assert_eq!(app.command_drops_for_test(), 0);
        assert_eq!(app.send_cmd_count_for_test(), capacity);

        app.send_cmd(ActorCommand::Lifecycle(
            LifecycleCommand::MarkChangedSinceEmit,
        ));

        assert_eq!(
            app.queue_depth_for_test(),
            capacity,
            "dropped commands must not inflate queue depth"
        );
        assert_eq!(app.command_drops_for_test(), 1);
        assert_eq!(
            app.send_cmd_count_for_test(),
            capacity,
            "test send count tracks accepted commands, not dropped attempts"
        );
    }
}
