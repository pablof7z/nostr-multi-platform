//! `ActorCommand` — the actor inbox command type (ADR-0065).
//!
//! The top-level enum has 11 variants, each carrying a sub-payload enum
//! grouped by cohesive ownership. The families match the existing
//! `actor/dispatch/cmd_*.rs` dispatch split (D4: one dispatch authority, one
//! type authority). See `docs/decisions/0065-actor-command-sub-enum-collapse.md`
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
pub use super::commands::relay_command::RelayCommand;
pub use super::commands::refs_command::RefsCommand;
pub use super::commands::sign_command::SignCommand;
#[cfg(any(test, feature = "test-support"))]
pub use super::commands::test_support_command::TestSupportCommand;

/// Where a signer added via [`IdentityCommand::AddSigner`] comes from.
///
/// Replaces the per-source `SignInNsec` / `SignInBunker` / `AddRemoteSigner`
/// command split: the source kind is now a payload of one unified command.
///
/// D0: the `RemoteHandle` arm carries a `Box<dyn RemoteSignerHandle>` whose
/// concrete type lives in `nmp-signers` — `nmp-core` only sees the trait object
/// (defined in [`crate::remote_signer`]); it never imports the broker or
/// signer crate.
#[allow(dead_code)] // live cross-crate constructors in nmp-ffi — per-crate lint false positive
pub enum SignerSource {
    /// Local secret key — a `nsec1…` bech32 or 64-hex string. Resolves
    /// synchronously: the actor parses it and (when `make_active`) activates
    /// it immediately. Carried as [`zeroize::Zeroizing<String>`] so the
    /// plaintext secret is wiped from memory the instant the command is
    /// dropped — the in-flight window between FFI ingest and key parsing is
    /// minimized.
    LocalNsec(zeroize::Zeroizing<String>),
    /// Local secret key for an app-managed signer slot. The actor registers
    /// it in the signer roster, persists it as app-managed local material,
    /// and keeps it hidden from user account projections and active-account
    /// switching.
    AppManagedLocalNsec(zeroize::Zeroizing<String>),
    /// NIP-46 `bunker://` URI. Triggers an asynchronous broker handshake: the
    /// actor seeds the `bunker_handshake` projection, stashes `make_active`,
    /// and delegates the connect/get_public_key dance to the registered
    /// broker. The broker reports completion by sending back an `AddSigner`
    /// carrying a [`SignerSource::RemoteHandle`].
    BunkerUri(String),
    /// A fully-handshaken remote signer handle. The broker adapter constructs
    /// this after a NIP-46 handshake completes and sends it back to the actor,
    /// which inserts it into `IdentityRuntime.remote_signers` and applies
    /// `make_active` (the value the originating `BunkerUri` command stashed).
    RemoteHandle(Box<dyn crate::RemoteSignerHandle>),
}

impl std::fmt::Debug for SignerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret: `LocalNsec` redacts its payload. `Box<dyn
        // RemoteSignerHandle>` is not `Debug`, so `RemoteHandle` prints only
        // its discriminant + the handle's pubkey.
        match self {
            SignerSource::LocalNsec(_) => f.write_str("LocalNsec(<redacted>)"),
            SignerSource::AppManagedLocalNsec(_) => f.write_str("AppManagedLocalNsec(<redacted>)"),
            SignerSource::BunkerUri(uri) => f.debug_tuple("BunkerUri").field(uri).finish(),
            SignerSource::RemoteHandle(handle) => f
                .debug_tuple("RemoteHandle")
                .field(&handle.pubkey_hex())
                .finish(),
        }
    }
}

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
    ShowToast {
        message: String,
    },
    /// D6 + issue #1682 — surface a structured error [`UiToken`] from an
    /// off-actor worker thread (e.g. the NIP-17 gift-wrap publish
    /// continuation), which holds only a `CommandSender`, not a kernel
    /// reference. The actor thread routes it to `kernel.set_last_error_token`,
    /// writing both the machine `code` (`last_error_category`) and the
    /// English fallback prose (`last_error_toast`) so the shell can render
    /// localized prose.
    ShowErrorToken {
        token: crate::ui_token::UiToken,
    },
    /// Test-support-only actor verbs (cfg-gated). See [`TestSupportCommand`].
    #[cfg(any(test, feature = "test-support"))]
    TestSupport(TestSupportCommand),
}