import Foundation
import os.log

private let kmCmdLog = Logger(subsystem: "io.f7z.chirp", category: "KernelModel")

// ── T66a command surface (identity / publish / multi-account / relay / wallet)
// Every method is a pass-through to a real kernel dispatch. No Swift-side
// business logic, no cached state (D5/D8) — every accessor above is a
// pure read of the kernel snapshot.

@MainActor
extension KernelModel {

    /// Add a local-key (nsec) signer. Mirrors the Rust `add_signer(source:
    /// SignerSource::LocalNsec, make_active:)` API. The nsec path routes
    /// through the Chirp/Marmot identity FFI so the MLS registration
    /// side-effect is preserved (a bare `add_signer` on `NmpApp` would not
    /// register Marmot). `makeActive` is plumbed for API parity; the current
    /// Chirp identity FFI always activates the imported account.
    func addSigner(localNsec secret: String, makeActive: Bool = true) {
        kmCmdLog.info("addSigner(localNsec) dispatched (len=\(secret.count), makeActive=\(makeActive))")
        kernel.signInNsecAndRegisterMarmot(secret)
    }

    /// Add a NIP-46 remote (bunker) signer. Mirrors the Rust `add_signer(source:
    /// SignerSource::BunkerUri, make_active:)` API. Flows through the signer
    /// broker, which drives the connect handshake and emits
    /// `AddSigner(source: RemoteHandle, make_active:)`.
    func addSigner(bunkerUri uri: String, makeActive: Bool = true) {
        kernel.signInBunker(uri)
    }

    /// Cancel an in-flight NIP-46 handshake. The handshake projection rides the
    /// `typedBunkerHandshake` sidecar, so reading `bunkerHandshake` reconciles
    /// automatically when the broker emits `idle` on the next tick.
    func cancelBunkerHandshake() { kernel.cancelBunkerHandshake() }

    func nostrConnectURI() -> String? {
        // Chirp registers `chirp://` as a custom URL scheme (Info.plist
        // `CFBundleURLTypes`); the signer app deep-links back to
        // `chirp://nip46?...` on approval (handled in `ChirpApp.onOpenURL`).
        // Rust chooses the relay and composes the protocol URL; Swift only
        // supplies the platform callback route.
        return kernel.nostrConnectURI(callbackScheme: "chirp://nip46")
    }

    func createAccount(
        profile: [String: String] = ["name": "New User"],
        relays: [(String, String)]? = nil,
        mls: Bool = true
    ) {
        kmCmdLog.info("createAccount dispatched")
        let relayFacts = relays ?? onboardingRelayOverride.map { [($0, "")] } ?? []
        marmotRegistrationRequested = mls
        // PR-L: the bridge defends the JSON encode path instead of trapping
        // with `try!`. A typed-but-impossible encode failure surfaces as a
        // toast and the dispatch is aborted — never a crash.
        if let encodeError = kernel.createAccount(profile: profile, relays: relayFacts, mls: mls) {
            kmCmdLog.error("createAccount encode failed: \(encodeError, privacy: .public)")
            lastDispatchError = encodeError
            lastErrorToast = encodeError
            marmotRegistrationRequested = false
        }
    }

    @discardableResult
    func publishProfile(name: String, about: String, picture: String) -> DispatchResult {
        return track(kernel.publishProfile(name: name, about: about, picture: picture))
    }

    func switchActive(_ identityID: String) {
        marmotRegistrationRequested = true
        kernel.switchActive(identityID: identityID)
    }

    func removeAccount(_ identityID: String) {
        kernel.removeAccountAndForgetSecret(identityID: identityID)
    }

    @discardableResult
    func publishNote(_ content: String, replyTo: ChirpReplyTarget? = nil) -> DispatchResult {
        track(kernel.publishNote(content: content, replyTo: replyTo))
    }

    func retryPublish(handle: String) { kernel.retryPublish(handle: handle) }
    // S7/#1754: cancel addresses the operation correlation_id. The outbox row's
    // publish handle is accepted too (the kernel index self-maps it).
    func cancelPublish(correlationID: String) { kernel.cancelPublish(correlationID: correlationID) }

    @discardableResult
    func react(targetEventID: String, reaction: String = "❤") -> DispatchResult {
        track(kernel.react(targetEventID: targetEventID, reaction: reaction))
    }

    @discardableResult
    func repost(eventID: String, authorPubkey: String) -> DispatchResult {
        track(kernel.repost(eventID: eventID, authorPubkey: authorPubkey))
    }

    func claimVisibleNoteRelations(eventID: String) {
        kernel.claimVisibleNoteRelations(eventID: eventID)
    }

    func releaseVisibleNoteRelations(eventID: String) {
        kernel.releaseVisibleNoteRelations(eventID: eventID)
    }

    @discardableResult
    func follow(_ pubkey: String) -> DispatchResult {
        track(kernel.follow(pubkey: pubkey))
    }

    @discardableResult
    func unfollow(_ pubkey: String) -> DispatchResult {
        track(kernel.unfollow(pubkey: pubkey))
    }

    /// Dispatch `nmp.nip51.block_relay` for the active account (M14-1 / #2145).
    ///
    /// Reads the active account pubkey from `activeAccount` and includes it in
    /// the `BlockRelayInput` body so the router-owned ActionModule can read the
    /// current blocked set for idempotency. Fails immediately when no account
    /// is active (no spinner is started, no FFI call is made).
    @discardableResult
    func blockRelay(url: String) -> DispatchResult {
        guard let pubkey = activeAccount else {
            return .failure("block relay: no active account")
        }
        return track(kernel.blockRelay(url: url, accountPubkey: pubkey))
    }

    /// Dispatch `nmp.nip51.unblock_relay` for the active account (M14-1 / #2145).
    ///
    /// Symmetric to `blockRelay`. The router-owned ActionModule rejects with a
    /// Conflict (no publish) when the relay is not currently blocked.
    @discardableResult
    func unblockRelay(url: String) -> DispatchResult {
        guard let pubkey = activeAccount else {
            return .failure("unblock relay: no active account")
        }
        return track(kernel.unblockRelay(url: url, accountPubkey: pubkey))
    }

    /// Dispatch a NIP-57 zap through the `nmp.nip57.zap` ActionModule.
    /// The recipient's `lnurl` is sourced from the keyed profile sidecar
    /// (pre-extracted from kind:0 by Rust — the shell never parses metadata).
    ///
    /// V-106: `amountMsats` is required — there is no hardcoded default. The
    /// host surfaces `ZapAmountSheet` to let the user pick the amount (preset
    /// or custom), and passes the chosen msats here. This removes the old
    /// "every zap is 21 sats" behaviour.
    ///
    /// V-07: relay selection is kernel policy. We pass an empty `relays`
    /// list; the actor auto-selects from the recipient's kind:10002
    /// (NIP-65) write/both relays via `kernel.author_write_relays`. The
    /// shell never decides where the LN provider should publish the
    /// kind:9735 receipt.
    func zap(
        targetEventID: String,
        authorPubkey: String,
        lnurl: String,
        amountMsats: UInt64,
        comment: String? = nil
    ) -> DispatchResult {
        return track(
            kernel.zap(
                targetEventID: targetEventID,
                authorPubkey: authorPubkey,
                lnurl: lnurl,
                amountMsats: amountMsats,
                comment: comment
            )
        )
    }

    @discardableResult
    func createPublicGroup(group: GroupId, name: String, about: String?) -> DispatchResult {
        let result = track(kernel.createPublicGroup(group: group, name: name, about: about))
        if case .accepted = result {
            groupChat = GroupChatStore(groupId: group, kernel: kernel)
        }
        return result
    }

    /// V5 thin-shell: read the kernel's `action_lifecycle` projection for
    /// a given correlation_id's terminal entry. Returns `nil` when the
    /// kernel has no terminal recorded (either still in flight or
    /// dropped past the TTL window). The kernel handles all the
    /// retention bookkeeping — Swift does NOT track pending sets, NOT
    /// cache terminal stages locally, NOT acknowledge anything.
    func recentTerminal(correlationId: String) -> ActionLifecycleEntry? {
        actionLifecycle?.recentTerminal.first { $0.correlationId == correlationId }
    }

    /// V5 thin-shell: read the kernel's `action_lifecycle` projection for
    /// a given correlation_id's in-flight entry. Returns `nil` when the
    /// action either has not been dispatched, has already settled, or
    /// the kernel has not yet recorded its first stage.
    func inFlight(correlationId: String) -> ActionLifecycleEntry? {
        actionLifecycle?.inFlight.first { $0.correlationId == correlationId }
    }

    func clearDispatchError() { lastDispatchError = nil }

    /// V5 thin-shell: route a `DispatchResult` only through the
    /// synchronous-rejection slot. Successful dispatches surface entirely
    /// through the Rust-owned `action_lifecycle` projection — there is no
    /// Swift-side pending-actions set to populate.
    @discardableResult
    func track(_ result: DispatchResult) -> DispatchResult {
        if case let .failure(message) = result {
            kmCmdLog.error("dispatch_action rejected: \(message, privacy: .public)")
            lastDispatchError = message
        }
        return result
    }

    func addRelay(url: String, role: String) { kernel.addRelay(url: url, role: role) }
    func removeRelay(url: String) { kernel.removeRelay(url: url) }
    @discardableResult
    func publishDmRelayList(relays: [String]) -> DispatchResult {
        track(kernel.publishDmRelayList(relays: relays))
    }
    @discardableResult
    func publishRelayList(relays: [AppRelay]) -> DispatchResult {
        track(kernel.publishRelayList(relays: relays))
    }
    func openTimeline() { kernel.openTimeline() }
    func clearErrorToast() {
        lastErrorToast = nil
        lastErrorCategory = nil
    }

    /// Localized user-facing error prose for the current error toast
    /// (issue #1682). The shell OWNS the prose: it maps the Rust-supplied
    /// stable machine code (`lastErrorCategory`) to localized copy. Codes the
    /// shell does not recognize (e.g. relay-CLOSED categories, or any
    /// post-dated Rust code) fall back to the Rust English `lastErrorToast`.
    /// `nil` ⇒ no error toast on screen.
    var localizedErrorToast: String? {
        guard let toast = lastErrorToast else { return nil }
        guard let code = lastErrorCategory else { return toast }
        return UiErrorProse.localized(code: code) ?? toast
    }
    func showSuccessToast(_ message: String) { lastSuccessToast = message }
    func clearSuccessToast() { lastSuccessToast = nil }

    // ── NIP-47 wallet commands ────────────────────────────────────────────

    func walletConnect(uri: String) { kernel.walletConnect(uri: uri) }
    func walletDisconnect() { kernel.walletDisconnect() }
    func walletPayInvoice(bolt11: String, amountMsats: UInt64? = nil) {
        kernel.walletPayInvoice(bolt11: bolt11, amountMsats: amountMsats)
    }

    // ── T118 / G3 — scenePhase pass-through ───────────────────────────────

    func lifecycleForeground() { kernel.lifecycleForeground() }
    func lifecycleBackground() { kernel.lifecycleBackground() }
}
