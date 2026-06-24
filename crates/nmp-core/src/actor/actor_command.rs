//! `ActorCommand` — the actor inbox command type (ADR-0065).
//!
//! The top-level enum routes by command-family payloads plus a few small
//! singleton seams. The families match the actor dispatch ownership split (D4:
//! one dispatch authority, one type authority). See
//! `docs/decisions/0065-actor-command-sub-enum-collapse.md` for the full
//! rationale.
//!
//! These items are public under `nmp_core::actor::...` because
//! protocol/default crates still construct raw actor commands directly. Prefer
//! typed `CommandSender` helpers where they exist; ADR-0065 narrows the raw bus
//! shape, it does not make raw command construction the preferred application
//! API.

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
pub use super::signer_source::SignerSource;

/// The single waking-inbox command type for the actor (ADR-0050 §D3a).
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
    /// ADR-0050 signer-session capability port verbs: `sign |
    /// nip44_encrypt | nip44_decrypt`. Backend-transparent: local keys
    /// resolve inline, remote signers park under the continuation sink.
    Sign(SignCommand),
    /// Sign-and-publish verbs + publish-engine control (retry / cancel).
    Publish(PublishCommand),
    /// Active-account kind:3 follow-set mutations.
    Contacts(ContactsCommand),
    /// Relay-list edits + transport-layer control.
    Relay(RelayCommand),
    /// Refcounted reference-resolution verbs (ADR-0063 unified + legacy).
    Refs(RefsCommand),
    /// Subscription-registry verbs + pull cursors (ADR-0042 M2 / ADR-0058).
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
    /// D6 — surface an error toast from the FFI boundary. Used when the FFI
    /// layer detects a malformed argument (e.g. unparseable JSON) and cannot
    /// call `kernel.set_last_error_toast` directly (the FFI only has a channel
    /// sender, not a kernel reference). The actor thread receives this command
    /// and routes it to `kernel.set_last_error_toast` so the error becomes
    /// observable state, never a silent no-op.
    ShowToast { message: String },
    /// D6 + issue #1682 — surface a structured error [`UiToken`] from an
    /// off-actor worker thread (e.g. the NIP-17 gift-wrap publish
    /// continuation), which holds only a `CommandSender`, not a kernel
    /// reference. The actor thread routes it to `kernel.set_last_error_token`,
    /// writing both the machine `code` (`last_error_category`) and the
    /// English fallback prose (`last_error_toast`) so the shell can render
    /// localized prose.
    ShowErrorToken { token: crate::ui_token::UiToken },
    /// Test-support-only actor verbs (cfg-gated). See [`TestSupportCommand`].
    #[cfg(any(test, feature = "test-support"))]
    TestSupport(TestSupportCommand),
}
