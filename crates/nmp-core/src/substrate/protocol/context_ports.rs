use super::*;
use crate::actor::{ActionLedgerCommand, ActorCommand, InterestsCommand, PublishCommand};
use crate::relay::OutboundMessage;

impl<'a> ProtocolCommandContext<'a> {
    /// ADR-0043 Decision 2 — the generic, backend-transparent sign-account
    /// port helper. Build an [`ActorCommand::SignEventForAccount`] for
    /// `unsigned` (signed with the active account when `signer_pubkey` is
    /// `None`, else the named roster key) carrying `continuation`, and send it
    /// back into the actor loop.
    ///
    /// Local-vs-bunker is invisible to the caller: the actor's dispatch arm
    /// resolves a local key inline and parks a NIP-46 bunker op; the
    /// continuation is invoked on the actor thread either way, with the
    /// resolved [`SignedEvent`] or an error string. The continuation must only
    /// enqueue further work (e.g. spawn an HTTP worker), never block (D8). It
    /// never receives raw key bytes (D13).
    ///
    /// A worker thread that already holds a [`command_sender_clone`] should use
    /// [`build_sign_event_for_account`] instead — this method exists for command
    /// bodies that still hold the `ctx` on the actor thread.
    pub fn sign_event_for_account(
        &self,
        unsigned: nmp_signer_iface::UnsignedEvent,
        signer_pubkey: Option<String>,
        continuation: impl FnOnce(Result<nmp_signer_iface::SignedEvent, String>) + Send + 'static,
    ) {
        self.send(build_sign_event_for_account(
            unsigned,
            signer_pubkey,
            continuation,
        ));
    }

    /// ADR-0052 §D5 — borrow the narrow [`WalletKernelAccess`] capability (the
    /// NIP-47 wallet runtime's bounded kernel-mutation surface). Replaces the
    /// deleted `kernel_mut()`: a wallet command drives its nine kernel methods
    /// and nothing else of the kernel.
    #[must_use]
    pub fn wallet_kernel(&self) -> &dyn WalletKernelAccess {
        self.wallet_kernel
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

    /// Borrow the [`DmInboxRelayLookup`] capability.
    #[must_use]
    pub fn dms(&self) -> &dyn DmInboxRelayLookup {
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

    /// ADR-0052 §D4 — clone the configured host-op handler (`None` when none
    /// was installed before actor start). D15-wrapped: a panicking accessor
    /// falls back to `None` (the genuinely-absent-handler
    /// branch) rather than unwinding the calling `ProtocolCommand::run` frame.
    #[must_use]
    pub fn host_op_handler(&self) -> Option<std::sync::Arc<dyn crate::substrate::HostOpHandler>> {
        let h = self.host_op_handler;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.current_handler()))
            .unwrap_or(None)
    }

    // ── D15 catch_unwind shortcuts ──
    //
    // The accessors below wrap a capability call in `catch_unwind` so a
    // panicking host-side adapter cannot unwind the calling
    // `ProtocolCommand::run` frame. NIP commands MAY call the capability
    // method directly via `ctx.clock().now_secs()` etc., but these
    // shortcuts make the panic-safety explicit at the call site (every
    // previous accessor had a `catch_unwind` wrapper; the shortcuts
    // preserve that contract).

    /// Wall-clock seconds since the Unix epoch (D15-wrapped
    /// [`KernelClock::now_secs`]). Returns `0` on a panicking adapter.
    ///
    /// ADR-0052 §D5: always goes through the [`KernelClock`] capability — the
    /// prior kernel-direct fast-path (which dodged the now-deleted `with_kernel`
    /// exclusive borrow) is gone.
    pub fn now_secs(&self) -> u64 {
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
    /// [`ActorCommand::Nip44EncryptForAccount`] for `plaintext` → `peer_pubkey`
    /// (named `Some(hex)` or active `None` account). Local-vs-bunker is invisible
    /// (D13 — only ciphertext crosses); the continuation runs on the actor thread
    /// and only enqueues work (D8). Worker threads holding a `command_sender_clone`
    /// use [`build_nip44_encrypt_for_account`] directly.
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

    /// D15-wrapped [`DmInboxRelayLookup::dm_inbox_relays`]. Returns `None`
    /// on a panicking adapter (the gift-wrap publish path fails closed
    /// on `None` per NIP-17 § 2).
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

    /// D15-wrapped [`ErrorSurface::set_last_error_token`] (issue #1682) — emit a
    /// structured error token (machine `code` + English fallback prose) so the
    /// shell renders localized prose.
    pub fn set_last_error_token(&self, token: &crate::ui_token::UiToken) {
        let e = self.errors;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            e.set_last_error_token(token);
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
    /// Returns an empty `Vec` on a panicking adapter — matches the
    /// "router returned `Unroutable`" branch (caller decides how to
    /// fall back further).
    #[must_use]
    pub fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        let r = self.recipients;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            r.recipient_publish_relays(recipient, kind)
        }))
        .unwrap_or_default()
    }

    /// ADR-0052 §D5 — borrow the [`ZapProfileLookup`] capability (the zap-only
    /// cached-profile read). Replaces the deleted generic `lnurl_for_pubkey`
    /// accessor; the NIP-57 zap command reads its destination via
    /// `ctx.zap_profiles().lnurl_for_pubkey(pk)`, and no other command can.
    #[must_use]
    pub fn zap_profiles(&self) -> &dyn ZapProfileLookup {
        self.zap_profiles
    }

    // ── #1721 slice 3a — typed dispatch ports ──────────────────────────────
    // Thin wrappers so `ProtocolCommand::run` bodies never name `ActorCommand`
    // directly. Each delegates to `self.send(...)` (D15 isolation already there).

    /// Re-enter the actor loop via `ActorCommand::Protocol`.
    pub fn dispatch_protocol(&self, cmd: Box<dyn ProtocolCommand>) {
        self.send(ActorCommand::Protocol(cmd));
    }

    /// Publish unsigned via NIP-65 outbox (`ActorCommand::PublishUnsignedEvent`).
    /// `correlation_id` threads the action id; `signer_pubkey` overrides the
    /// active account when `Some(hex)`.
    pub fn publish_unsigned(
        &self,
        event: nmp_signer_iface::UnsignedEvent,
        correlation_id: Option<String>,
        signer_pubkey: Option<String>,
    ) {
        self.send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            signer_pubkey,
        }));
    }

    /// Publish unsigned to an explicit relay set, bypassing NIP-65 outbox
    /// (`ActorCommand::PublishUnsignedEventToRelays`).
    pub fn publish_unsigned_to_relays(
        &self,
        event: nmp_signer_iface::UnsignedEvent,
        relays: Vec<crate::publish::RelayUrl>,
        route_class: crate::publish::PublishRouteClass,
        correlation_id: Option<String>,
        signer_pubkey: Option<String>,
    ) {
        self.send(ActorCommand::Publish(
            PublishCommand::UnsignedEventToRelays {
                event,
                relays,
                route_class,
                correlation_id,
                signer_pubkey,
            },
        ));
    }

    /// Publish an already-signed event to an explicit relay set.
    ///
    /// This is the narrow protocol-command port for crates that own a
    /// protocol-specific signer or envelope and therefore cannot route through
    /// `PublishUnsignedEvent`. The command provides the verbatim signed event;
    /// the actor still owns target validation, publish retry state, ACK
    /// handling, and failure surfacing.
    pub fn publish_signed_to_relays(
        &self,
        event: nostr::Event,
        relays: Vec<crate::publish::RelayUrl>,
        route_class: crate::publish::PublishRouteClass,
        correlation_id: Option<String>,
    ) {
        let raw = nmp_store::RawEvent {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: u32::from(event.kind.as_u16()),
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            sig: event.sig.to_string(),
        };
        self.send(ActorCommand::Publish(PublishCommand::SignedEvent {
            raw,
            target: crate::publish::PublishTarget::explicit(relays, route_class),
            correlation_id,
        }));
    }

    /// Record a terminal `Accepted` stage (`ActorCommand::RecordActionSuccess`).
    /// `result_json` is the optional Decision-4 structured return payload.
    pub fn record_action_success(&self, correlation_id: String, result_json: Option<String>) {
        self.send(ActorCommand::ActionLedger(
            ActionLedgerCommand::RecordSuccess {
                correlation_id,
                result_json,
            },
        ));
    }

    pub fn ensure_interest(
        &self,
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
    ) {
        self.send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
            identity,
            interest,
        }));
    }

    pub fn drop_interest_owner(&self, identity: crate::subs::SubIdentity) {
        self.send(ActorCommand::Interests(
            InterestsCommand::DropInterestOwner(identity),
        ));
    }
}
