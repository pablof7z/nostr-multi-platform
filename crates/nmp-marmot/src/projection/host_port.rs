//! `MarmotHostPort` — the single outbound-effect seam Marmot ops use.
//!
//! # Why this exists (#1940)
//!
//! Before #1940 the dispatch ops reached the host through a stored
//! `*mut NmpApp` raw pointer (`InnerHandle::app()`): publish routed through
//! `nmp_ffi::NmpApp::publish_signed_explicit`, write-relay reads through
//! `NmpApp::write_relay_urls`, and interest enqueues through
//! `NmpApp::ensure_interest`. That coupled the PROTOCOL crate to `nmp-ffi`
//! and forced an `unsafe` raw-pointer deref on every op.
//!
//! `MarmotHostPort` replaces that pointer with a typed trait the CALLER
//! supplies. There is ONE trait and ONE set of outbound effects; two ways
//! to obtain a port:
//!
//! * [`crate::projection::command::ContextHostPort`] — built over a
//!   `nmp_core::substrate::ProtocolCommandContext` for the production
//!   command path (`MarmotProtocolCommand::run`). Reaches publish / write
//!   relays / interests through the kernel's typed capability seams.
//! * [`CommandSenderHostPort`] — built over a stored
//!   `nmp_core::actor::CommandSender` (+ a write-relay reader closure) for
//!   the tap-driven ingest / deferred-KP-retry paths, which run on the
//!   actor thread with no `ctx` in scope.
//!
//! Unit tests use [`NoopMarmotHostPort`] or a recording port, replacing the
//! old null-`NmpApp` test path.

use nostr::{Event, RelayUrl};

use nmp_core::actor::ActorCommand;
#[cfg(feature = "ffi")]
use nmp_core::actor::{CommandSender, InterestsCommand, PublishCommand};
use nmp_core::subs::SubIdentity;
use nmp_planner::LogicalInterest;

use crate::interest::KIND_GIFT_WRAP;

/// The single outbound-effect seam Marmot dispatch ops use. Every publish,
/// write-relay read, and interest enqueue goes through one of these so the
/// `ops` layer never names `NmpApp` or the kernel context type directly.
///
/// The trait itself carries NO `Send + Sync` bound so the command-path
/// [`crate::projection::command::ContextHostPort`] (which borrows the kernel's
/// `&ProtocolCommandContext`, holding a `&dyn Fn` send closure that is not
/// `Sync`) can implement it. The STORED variant is bounded at its storage
/// site: `Arc<dyn MarmotHostPort + Send + Sync>` (see
/// [`crate::projection::state`]), and [`CommandSenderHostPort`] is `Send +
/// Sync` (a `CommandSender` + a `Send + Sync` reader closure).
pub(crate) trait MarmotHostPort {
    /// Publish an ALREADY-SIGNED event to an EXPLICIT relay set. Carries the
    /// D10 kind:1059-empty-relays provenance guard (see [`publish_guarded`]).
    fn publish_signed_explicit(&self, event: &Event, relays: &[RelayUrl]);

    /// The user's NIP-65 write-relay URLs (empty when none configured).
    fn write_relay_urls(&self) -> Vec<String>;

    /// Attach one scoped owner to a `LogicalInterest` (KeyPackage fetch,
    /// group-message subscribe, etc.).
    fn ensure_interest(&self, identity: SubIdentity, interest: LogicalInterest);

    /// Re-enter the actor loop with `cmd` (deferred terminal verdicts /
    /// `RecordAction*`). Non-blocking (unbounded mpsc); a disconnected
    /// channel is a benign no-op (D6/D8).
    fn send_actor_command(&self, cmd: ActorCommand);
}

/// D10 provenance guard shared by every port impl: a kind:1059 gift-wrap with
/// NO explicit relay pin MUST NOT reach any publish path that could leak the
/// presence of an encrypted DM / Welcome to public relays. Returns `true` when
/// the publish is permitted, `false` when the guard blocks it.
///
/// Kept here (not inside an impl) so the gate is testable without a context
/// and every port impl applies the identical predicate.
#[must_use]
pub(crate) fn publish_permitted(event: &Event, relays: &[RelayUrl]) -> bool {
    !(event.kind.as_u16() as u32 == KIND_GIFT_WRAP && relays.is_empty())
}

/// A [`MarmotHostPort`] over a stored [`CommandSender`] plus a write-relay
/// reader. Used by the tap-driven ingest and deferred-KP-retry paths, which
/// run on the actor thread with no `ProtocolCommandContext` in scope.
///
/// `write_relays` is a boxed reader so this crate's PROTOCOL layer needs no
/// `nmp-ffi` dependency: the FFI register tail builds the closure over the
/// live app (where `nmp-ffi` is an allowed `ffi`-feature dep). Only the C-ABI
/// shell constructs this, so it is gated behind the `ffi` feature.
#[cfg(feature = "ffi")]
pub(crate) struct CommandSenderHostPort {
    sender: CommandSender,
    write_relays: Box<dyn Fn() -> Vec<String> + Send + Sync>,
}

#[cfg(feature = "ffi")]
impl CommandSenderHostPort {
    #[must_use]
    pub(crate) fn new(
        sender: CommandSender,
        write_relays: Box<dyn Fn() -> Vec<String> + Send + Sync>,
    ) -> Self {
        Self {
            sender,
            write_relays,
        }
    }
}

#[cfg(feature = "ffi")]
impl MarmotHostPort for CommandSenderHostPort {
    fn publish_signed_explicit(&self, event: &Event, relays: &[RelayUrl]) {
        if !publish_permitted(event, relays) {
            return;
        }
        let raw = raw_event_of(event);
        let relays: Vec<nmp_core::publish::RelayUrl> =
            relays.iter().map(std::string::ToString::to_string).collect();
        let _ = self
            .sender
            .send(ActorCommand::Publish(PublishCommand::SignedEvent {
                raw,
                target: nmp_core::publish::PublishTarget::Explicit { relays },
                correlation_id: None,
            }));
    }

    fn write_relay_urls(&self) -> Vec<String> {
        (self.write_relays)()
    }

    fn ensure_interest(&self, identity: SubIdentity, interest: LogicalInterest) {
        let _ = self
            .sender
            .send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
                identity,
                interest,
            }));
    }

    fn send_actor_command(&self, cmd: ActorCommand) {
        let _ = self.sender.send(cmd);
    }
}

/// Build the kernel `RawEvent` from a signed `nostr::Event` (the same shape
/// the kernel `SignedEvent` publish command carries). Used only by the
/// `ffi`-feature [`CommandSenderHostPort`].
#[cfg(feature = "ffi")]
pub(crate) fn raw_event_of(event: &Event) -> nmp_store::RawEvent {
    nmp_store::RawEvent {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        kind: u32::from(event.kind.as_u16()),
        tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: event.content.clone(),
        sig: event.sig.to_string(),
    }
}

/// No-op [`MarmotHostPort`]. Replaces the old null-`NmpApp` path (publish /
/// interest / write-relay / terminal-verdict all degrade to no-ops, matching
/// the D6 fire-and-forget contract). Used by unit tests that exercise pure
/// dispatch logic without asserting outbound effects, AND as the snapshot-edge
/// eviction fallback when no `CommandSender`-backed port is bound (the
/// in-memory test projection).
pub(crate) struct NoopMarmotHostPort;

impl MarmotHostPort for NoopMarmotHostPort {
    fn publish_signed_explicit(&self, _event: &Event, _relays: &[RelayUrl]) {}
    fn write_relay_urls(&self) -> Vec<String> {
        Vec::new()
    }
    fn ensure_interest(&self, _identity: SubIdentity, _interest: LogicalInterest) {}
    fn send_actor_command(&self, _cmd: ActorCommand) {}
}

#[cfg(test)]
mod tests {
    //! D10 provenance-guard predicate tests (migrated verbatim from the
    //! deleted `projection::publish` module — the guard is still load-bearing).
    use super::publish_permitted;
    use nostr::{Event, EventBuilder, Keys, Kind, RelayUrl};

    fn sample(kind: u16) -> Event {
        let keys = Keys::generate();
        EventBuilder::new(Kind::from_u16(kind), "")
            .sign_with_keys(&keys)
            .expect("test-only signing must succeed")
    }

    #[test]
    fn kind_1059_with_empty_relays_is_blocked() {
        assert!(
            !publish_permitted(&sample(1059), &[]),
            "kind:1059 + empty relays must be blocked by the D10 guard"
        );
    }

    #[test]
    fn kind_1059_with_explicit_relays_is_permitted() {
        let pin: Vec<RelayUrl> = vec!["wss://dm.example/".parse().expect("parse url")];
        assert!(
            publish_permitted(&sample(1059), &pin),
            "kind:1059 + explicit relays must pass the D10 guard"
        );
    }

    #[test]
    fn kind_445_with_empty_relays_is_not_d10_blocked() {
        assert!(
            publish_permitted(&sample(445), &[]),
            "kind:445 + empty relays is not a D10 gift-wrap leak"
        );
    }

    #[test]
    fn kind_30443_keypackage_with_empty_relays_is_not_d10_blocked() {
        assert!(
            publish_permitted(&sample(30443), &[]),
            "kind:30443 KeyPackage + empty relays is not a D10 gift-wrap leak"
        );
    }
}
