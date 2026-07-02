//! `ActorCommand` — the actor inbox command type (ADR-0071).
//!
//! The top-level enum has 11 variants, each carrying a sub-payload enum
//! grouped by cohesive ownership. The families match the existing
//! `actor/dispatch/cmd_*.rs` dispatch split (D4: one dispatch authority, one
//! type authority). See `docs/decisions/0071-write-intents-and-route-provenance.md`
//! for the full rationale.
//!
//! The `actor` module is private (`mod actor`, not `pub mod actor`), so this
//! `pub` is only reachable from outside the crate through the `testing`
//! re-export gate. In normal (non-test-support) builds nothing re-exports
//! these items, so they remain effectively crate-private.

use crate::app::KernelAction;

pub use super::commands::action_ledger_command::ActionLedgerCommand;
pub use super::commands::contacts_command::ContactsCommand;
pub use super::commands::identity_command::IdentityCommand;
pub use super::commands::interests_command::InterestsCommand;
pub use super::commands::lifecycle_command::LifecycleCommand;
pub use super::commands::publish_command::PublishCommand;
pub use super::commands::refs_command::RefsCommand;
pub use super::commands::relay_command::RelayCommand;
pub use super::commands::sign_command::SignCommand;
#[cfg(any(test, feature = "test-support"))]
pub use super::commands::test_support_command::TestSupportCommand;

// `SignerSource` lives in `actor/signer_source.rs` (extracted by #1903) and is
// re-exported through `actor/mod.rs` → `crate::SignerSource`. No duplication
// here.

/// The single waking-inbox command type for the actor (ADR-0072 §D3a).
///
/// Every host intent, every capability completion, every protocol-crate
/// write, and every test-support hook is one variant. The families group
/// cohesive verbs; the dispatch arm matches the family first, then the verb
/// (see `actor/dispatch/mod.rs`).
#[derive(Debug)]
pub enum ActorCommand {
    /// Actor + app lifecycle verbs: `Start` / `Configure` / `Stop` / `Reset` /
    /// `Shutdown` / `LifecycleEvent` / `MarkChangedSinceEmit`.
    Lifecycle(LifecycleCommand),
    /// Signer-roster management + account lifecycle + remote-signer health
    /// slots. These *mutate* the roster; the [`Self::Sign`] family *uses* it.
    Identity(IdentityCommand),
    /// ADR-0072 signer-session capability port verbs: `sign |
    /// nip44_encrypt | nip44_decrypt`. Backend-transparent: local keys
    /// resolve inline, remote signers park under the continuation sink.
    Sign(SignCommand),
    /// Sign-and-publish verbs + publish-engine control (retry / cancel).
    Publish(PublishCommand),
    /// Active-account kind:3 follow-set mutations.
    Contacts(ContactsCommand),
    /// Relay-list edits + transport-layer control.
    Relay(RelayCommand),
    /// Refcounted reference-resolution verbs (ADR-0070 unified + legacy).
    Refs(RefsCommand),
    /// Subscription-registry verbs + pull cursors (ADR-0076 M2 / ADR-0072).
    Interests(InterestsCommand),
    /// Action-stage ledger: host ACK + worker terminal recording.
    ActionLedger(ActionLedgerCommand),
    /// Open-seam command dispatched through the
    /// [`crate::substrate::ProtocolCommand`] trait. NIP crates use this
    /// instead of adding their own variant to `ActorCommand`
    /// (`docs/architecture/crate-boundaries.md` §4.1, step 1.b).
    Protocol(Box<dyn crate::substrate::ProtocolCommand>),
    /// Generic FFI-boundary action (T95). Routed through the
    /// [`dispatch_kernel_action`] reducer; the resolved [`KernelUpdate`] is
    /// serialized and pushed on the update channel. `OpenUri` registers the
    /// resolved interest through the single-writer registry (D4).
    Kernel(KernelAction),
    /// D6 — legacy raw-prose boundary ingress for callers that only have a
    /// command sender. New kernel/core producers must use `ShowErrorToken` so
    /// the snapshot carries a stable `last_error_category`; this variant remains
    /// for callers that have not yet defined token codes (e.g. the typed wrapper
    /// in `nmp-native-runtime::app_impl_core`).
    ShowToast { message: String },
    /// D6 + issue #1682 — surface a structured error [`UiToken`] from an
    /// off-actor worker thread (e.g. the NIP-17 gift-wrap publish
    /// continuation), which holds only a `CommandSender`, not a kernel
    /// reference. The actor thread routes it to `kernel.set_last_error_token`,
    /// writing both the machine `code` (`last_error_category`) and the
    /// English fallback prose (`last_error_toast`) so the shell can render
    /// localized prose.
    ShowErrorToken { token: crate::ui_token::UiToken },
    /// Enqueue a raw outbound text frame on `relay_url` from a
    /// [`crate::substrate::RelayConnectedHook`] (or any off-actor sender
    /// that cannot return `Vec<OutboundMessage>` directly). Fire-and-forget:
    /// the actor dispatches it to `send_outbound` without waiting for an
    /// acknowledgement. D0-clean — carries only substrate-generic types
    /// (`RelayRole`, `String`); no NIP protocol noun crosses this boundary.
    EnqueueOutbound {
        /// Lane discriminator — determines persistence and health-row placement.
        role: nmp_network::role::RelayRole,
        /// Canonical relay URL.
        relay_url: String,
        /// Fully-formed wire frame (e.g. `["REQ", ...]` or `["EVENT", ...]`).
        text: String,
    },
    /// Register a reconnect preamble with the relay worker for `relay_url`.
    ///
    /// The worker injects `frames` at the FRONT of its outbound `pending`
    /// queue immediately after every `RelayEvent::Connected`, before the
    /// actor's `Opened` hook can enqueue any `Send` commands.  This is the
    /// structural REQ-before-EVENT guarantee: a NIP-46 REQ registered here
    /// will always reach the wire before any sign EVENT queued by the hook.
    ///
    /// D0-clean: `frames` are opaque strings; no protocol noun in
    /// `nmp-network`.  The worker's `preamble` is owned by one caller;
    /// the last write wins.
    SetReconnectPreamble {
        /// Lane discriminator — identifies which pool slot to update.
        role: nmp_network::role::RelayRole,
        /// Canonical relay URL of the target worker.
        relay_url: String,
        /// Fully-formed wire frames to inject on every (re)connect.
        frames: Vec<String>,
    },
    /// Cancel a persistent NIP-46 subscription.
    ///
    /// Sent by the NIP-46 runtime teardown path when clearing a session (e.g.
    /// account removal). The actor thread removes `sub_id` from the persistent
    /// sub registry for `relay_url` so the relay worker no longer prevents
    /// EOSE-triggered CLOSE for this subscription.
    ///
    /// D0-clean: carries only generic strings; no NIP protocol noun.
    UnregisterPersistentSub {
        /// Canonical relay URL where the subscription was registered.
        relay_url: String,
        /// The subscription id (`"nip46-<pubkey_prefix>"` pattern).
        sub_id: String,
    },
    /// Test-support-only actor verbs (cfg-gated). See [`TestSupportCommand`].
    #[cfg(any(test, feature = "test-support"))]
    TestSupport(TestSupportCommand),
}
