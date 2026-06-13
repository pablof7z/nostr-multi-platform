//! `ProtocolCommandContext` impl — extracted from `substrate/protocol.rs` to
//! keep the parent file under its baseline LOC ceiling. Compiled inline via
//! `#[path = "protocol/context.rs"] mod context_impl;` in `protocol.rs`.

use super::{
    ActionStageTracker, DmInboxLookup, ErrorSurface, KernelClock, LocalSignerAccess,
    NoopActionStageTracker, NoopErrorSurface, NoopKernelClock, NoopLocalSignerAccess,
    NoopRecipientRelayLookup, ProtocolCommandContext, ProtocolCommandContextParts,
    RecipientRelayLookup, build_nip44_encrypt_for_account, build_sign_event_for_account,
};
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::ActorCommand;

impl<'a> ProtocolCommandContext<'a> {
    /// Construct from a [`ProtocolCommandContextParts`] bundle (the sole
    /// public production door). Capability references close over the
    /// dispatch arm's stack-bound borrows of kernel + identity runtime;
    /// the resulting context's lifetime is the dispatch arm's stack frame.
    ///
    /// V-38: `kernel` + `outbound` start as `None`; attach them via the
    /// [`with_kernel`](Self::with_kernel) / [`with_outbound`](Self::with_outbound)
    /// builders from the dispatch arm.
    pub fn new(parts: ProtocolCommandContextParts<'a>) -> Self {
        let ProtocolCommandContextParts {
            send,
            command_sender,
            clock,
            signers,
            dms,
            errors,
            stages,
            recipients,
        } = parts;
        Self {
            send,
            command_sender,
            clock,
            signers,
            dms,
            errors,
            stages,
            recipients,
            kernel: None,
            outbound: None,
        }
    }

    /// V-38 builder: attach the actor's kernel handle. The dispatch arm
    /// calls this before invoking [`ProtocolCommand::run`].
    #[must_use]
    pub fn with_kernel(mut self, kernel: &'a mut Kernel) -> Self {
        self.kernel = Some(kernel);
        self
    }

    /// V-38 builder: attach an outbound-frame sink so the command body can
    /// surface relay frames produced synchronously on the actor thread.
    #[must_use]
    pub fn with_outbound(mut self, outbound: &'a mut Vec<OutboundMessage>) -> Self {
        self.outbound = Some(outbound);
        self
    }

    /// Test-only constructor that wires only the [`send`](Self::send)
    /// closure. All capability accessors return harmless defaults (0,
    /// `None`, no-op) via the noop singletons; `command_sender_clone`
    /// returns a sender whose receiver is dropped (sends become benign
    /// no-ops, matching the D6 "disconnected actor" pattern). Tests
    /// needing a specific capability build a small local adapter and
    /// pass it through [`Self::new`] via a [`ProtocolCommandContextParts`]
    /// literal.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_send_only(send: &'a dyn Fn(ActorCommand)) -> Self {
        static CLOCK: NoopKernelClock = NoopKernelClock;
        static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
        static DMS: crate::substrate::EmptyDmInboxRelayLookup =
            crate::substrate::EmptyDmInboxRelayLookup;
        static ERRORS: NoopErrorSurface = NoopErrorSurface;
        static STAGES: NoopActionStageTracker = NoopActionStageTracker;
        static RECIPIENTS: NoopRecipientRelayLookup = NoopRecipientRelayLookup;
        let (command_sender, _rx) = std::sync::mpsc::channel::<crate::actor::ActorMail>();
        let command_sender = crate::actor::CommandSender::new(command_sender);
        Self::new(ProtocolCommandContextParts {
            send,
            command_sender,
            clock: &CLOCK,
            signers: &SIGNERS,
            dms: &DMS,
            errors: &ERRORS,
            stages: &STAGES,
            recipients: &RECIPIENTS,
        })
    }

    /// Return an owned [`CommandSender`](crate::actor::CommandSender) clone for
    /// handing to a spawned worker thread that posts follow-up `ActorCommand`s
    /// back into the actor loop after the dispatch arm (and therefore this
    /// `ProtocolCommandContext`) has returned — the LNURL fetcher pattern
    /// (`nmp_nip57::lnurl::FetchLnurlInvoiceCommand`). The test-only
    /// `with_send_only` ctor installs a sender whose receiver is dropped
    /// (sends become benign no-ops, matching D6).
    #[must_use]
    pub fn command_sender_clone(&self) -> crate::actor::CommandSender {
        self.command_sender.clone()
    }

    /// Re-enter the actor loop with `cmd`. D15: the host-supplied closure
    /// is wrapped in [`std::panic::catch_unwind`] so a panicking follow-up
    /// cannot unwind the calling `ProtocolCommand::run` frame.
    pub fn send(&self, cmd: ActorCommand) {
        let send = self.send;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| send(cmd)));
    }

    /// ADR-0043 Decision 2 — the generic, backend-transparent sign-account
    /// port helper. Build an [`ActorCommand::SignEventForAccount`] for
    /// `unsigned` (signed with the active account when `signer_pubkey` is
    /// `None`, else the named roster key) carrying `continuation`, and send it
    /// back into the actor loop.
    pub fn sign_event_for_account(
        &self,
        unsigned: crate::substrate::UnsignedEvent,
        signer_pubkey: Option<String>,
        continuation: impl FnOnce(Result<crate::substrate::SignedEvent, String>) + Send + 'static,
    ) {
        self.send(build_sign_event_for_account(
            unsigned,
            signer_pubkey,
            continuation,
        ));
    }

    /// V-38: Reborrow the actor's kernel handle. `None` only in unit tests
    /// that constructed the context without one.
    pub fn kernel_mut(&mut self) -> Option<&mut Kernel> {
        self.kernel.as_deref_mut()
    }

    /// V-38: Push outbound relay frames produced synchronously by the command
    /// body. The actor's dispatch arm drains them into the existing
    /// `send_all_outbound` plumbing. No-op when no outbound sink is attached
    /// (unit tests).
    pub fn push_outbound<I: IntoIterator<Item = OutboundMessage>>(&mut self, frames: I) {
        if let Some(out) = self.outbound.as_mut() {
            out.extend(frames);
        }
    }

    /// Borrow the [`KernelClock`] capability.
    #[must_use]
    pub fn clock(&self) -> &dyn KernelClock {
        self.clock
    }

    /// Borrow the [`LocalSignerAccess`] capability.
    #[must_use]
    pub fn signers(&self) -> &dyn LocalSignerAccess {
        self.signers
    }

    /// Borrow the [`DmInboxLookup`] capability.
    #[must_use]
    pub fn dms(&self) -> &dyn DmInboxLookup {
        self.dms
    }

    /// Borrow the [`ErrorSurface`] capability.
    #[must_use]
    pub fn errors(&self) -> &dyn ErrorSurface {
        self.errors
    }

    /// Borrow the [`ActionStageTracker`] capability.
    #[must_use]
    pub fn stages(&self) -> &dyn ActionStageTracker {
        self.stages
    }

    /// Borrow the [`RecipientRelayLookup`] capability.
    #[must_use]
    pub fn recipients(&self) -> &dyn RecipientRelayLookup {
        self.recipients
    }

    // ── D15 catch_unwind shortcuts ──

    /// Wall-clock seconds since the Unix epoch (D15-wrapped
    /// [`KernelClock::now_secs`]). Returns `0` on a panicking adapter.
    ///
    /// When a direct `&Kernel` is attached (production dispatch), reads the
    /// kernel's clock directly so the RefCell held by `KernelClockAdapter`
    /// in the dispatch arm cannot trigger a double-borrow panic.
    pub fn now_secs(&self) -> u64 {
        if let Some(k) = self.kernel.as_deref() {
            return k.now_secs();
        }
        let c = self.clock;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.now_secs())).unwrap_or(0)
    }

    /// D15-wrapped [`LocalSignerAccess::active_local_keys`]. Returns
    /// `None` on a panicking adapter (matches the genuinely-absent
    /// account branch).
    #[must_use]
    pub fn active_local_keys(&self) -> Option<nostr::Keys> {
        let s = self.signers;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.active_local_keys()))
            .unwrap_or(None)
    }

    /// D15-wrapped [`LocalSignerAccess::active_account_pubkey`] — the §D5
    /// account-pin source. Returns `None` on a panicking adapter (matches the
    /// genuinely-absent account branch).
    #[must_use]
    pub fn active_account_pubkey(&self) -> Option<String> {
        let s = self.signers;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.active_account_pubkey()))
            .unwrap_or(None)
    }

    /// ADR-0050 §D1 cipher-port helper — the NIP-44 encrypt twin of
    /// [`sign_event_for_account`](Self::sign_event_for_account). Sends an
    /// [`ActorCommand::Nip44EncryptForAccount`] for `plaintext` → `peer_pubkey`.
    pub fn nip44_encrypt_for_account(
        &self,
        peer_pubkey: String,
        plaintext: String,
        signer_pubkey: Option<String>,
        continuation: impl FnOnce(Result<String, String>) + Send + 'static,
    ) {
        self.send(build_nip44_encrypt_for_account(
            peer_pubkey,
            plaintext,
            signer_pubkey,
            continuation,
        ));
    }

    /// D15-wrapped [`DmInboxLookup::dm_inbox_relays`]. Returns `None`
    /// on a panicking adapter.
    #[must_use]
    pub fn dm_inbox_relays(&self, recipient: &str) -> Option<Vec<String>> {
        let d = self.dms;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            d.dm_inbox_relays(recipient)
        }))
        .unwrap_or(None)
    }

    /// D15-wrapped [`ErrorSurface::set_last_error_toast`].
    pub fn set_last_error_toast(&self, message: Option<String>) {
        let e = self.errors;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            e.set_last_error_toast(message);
        }));
    }

    /// D15-wrapped [`ErrorSurface::record_action_failure`].
    pub fn record_action_failure(&self, correlation_id: String, reason: String) {
        let e = self.errors;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            e.record_action_failure(correlation_id, reason);
        }));
    }

    /// D15-wrapped [`ActionStageTracker::record_requested`].
    pub fn record_action_stage_requested(&self, correlation_id: &str) {
        let s = self.stages;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.record_requested(correlation_id);
        }));
    }

    /// D15-wrapped [`RecipientRelayLookup::recipient_publish_relays`].
    /// Returns an empty `Vec` on a panicking adapter.
    #[must_use]
    pub fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        let r = self.recipients;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            r.recipient_publish_relays(recipient, kind)
        }))
        .unwrap_or_default()
    }

    /// Return the lightning address / LNURL from the author's cached kind:0
    /// profile. `None` when the kernel is unavailable, the profile hasn't
    /// arrived yet, or the profile has no lightning address.
    #[must_use]
    pub fn lnurl_for_pubkey(&self, pubkey: &str) -> Option<String> {
        let kernel = self.kernel.as_deref()?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            kernel.lnurl_for_pubkey(pubkey)
        }))
        .ok()
        .flatten()
    }
}
