import Foundation

// ── Identity / account / signer operations ───────────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

extension KernelHandle {
    // ── T66a identity / publish / multi-account / relay-edit ──────────────

    // NOTE: the local-nsec sign-in path does NOT go through `nmp_app_signin_nsec`
    // here — `KernelModel.addSigner(localNsec:)` routes through
    // `MarmotBridge.signInNsecAndRegisterMarmot` (the Chirp/Marmot identity FFI)
    // so the MLS registration side-effect is preserved. The bare
    // `nmp_app_signin_nsec` wrapper that used to live here had no callers and was
    // removed when the Rust `add_signer` redesign landed.

    // Compatibility C ABI: the stable `nmp_app_signin_bunker` symbol now routes
    // through Rust's unified `AddSigner { source: BunkerUri, .. }` command.
    // Swift keeps the old symbol name so shipped shells do not need a header
    // churn for the internal actor-command rename.
    func signInBunker(_ uri: String) {
        uri.withCString { nmp_app_signin_bunker(raw, $0, 1) }
    }

    /// Cancel an in-flight NIP-46 bunker handshake. Idempotent / safe when
    /// nothing is in flight (no-op).
    func cancelBunkerHandshake() {
        nmp_app_cancel_bunker_handshake(raw)
    }

    /// Generate a fresh `nostrconnect://` URI for the QR-code NIP-46 sign-in
    /// flow. Returns `nil` if the broker is not yet initialised (which would
    /// be unusual — it's init'd in `KernelHandle.init()`). Each call produces
    /// a new ephemeral keypair and session secret.
    ///
    /// `callbackScheme` is the deep-link URL the signer app should open after
    /// approval (e.g. `"chirp://nip46"`). Rust chooses the relay from the
    /// kernel relay projection, percent-encodes the callback, and appends the
    /// `&callback=` query parameter. Swift supplies only platform callback
    /// information.
    func nostrConnectURI(callbackScheme: String? = nil) -> String? {
        if let cb = callbackScheme {
            return cb.withCString { cbPtr in
                guard let ptr = nmp_app_nostrconnect_uri(raw, cbPtr) else {
                    return nil
                }
                defer { nmp_free_string(ptr) }
                return String(cString: ptr)
            }
        }
        guard let ptr = nmp_app_nostrconnect_uri(raw, nil) else {
            return nil
        }
        defer { nmp_free_string(ptr) }
        return String(cString: ptr)
    }

    /// Dispatch a `nmp_app_chirp_create_new_account` call.
    ///
    /// Uses the Chirp-owned wrapper (not the generic `nmp_app_create_new_account`)
    /// so the fresh account auto-follows Chirp's product seed set, which lives in
    /// Rust (`nmp_chirp_config::chirp_default_follows`) — the seed pubkeys never
    /// transit this shell (#1493).
    ///
    /// The profile + relays are encoded through the `CreateAccountFFIPayload`
    /// `Codable` struct so the exact wire shape (`{"name":"…"}` + `[[url,role],…]`)
    /// is preserved while the encode path stays typed and defensible.
    ///
    /// Returns `nil` on success. Returns a human-readable error string on
    /// JSON-encode failure (typed-but-impossible for the `[String:String]` /
    /// `[(String,String)]` shapes we accept here, but we defend the boundary
    /// rather than trap with `try!`). Callers (`KernelModel.createAccount`)
    /// surface the error through the dispatch-error toast slot and abort the
    /// dispatch instead of crashing.
    @discardableResult
    func createAccount(
        profile: [String: String],
        relays: [(String, String)],
        mls: Bool = true
    ) -> String? {
        let payload = CreateAccountFFIPayload(profile: profile, relays: relays)
        let encoder = JSONEncoder()
        let profileStr: String
        let relaysStr: String
        do {
            let profileData = try encoder.encode(payload.profile)
            guard let str = String(data: profileData, encoding: .utf8) else {
                return "createAccount: failed to encode profile JSON as UTF-8"
            }
            profileStr = str
        } catch {
            return "createAccount: failed to encode profile (\(error.localizedDescription))"
        }
        do {
            let relaysData = try encoder.encode(payload.relays)
            guard let str = String(data: relaysData, encoding: .utf8) else {
                return "createAccount: failed to encode relays JSON as UTF-8"
            }
            relaysStr = str
        } catch {
            return "createAccount: failed to encode relays (\(error.localizedDescription))"
        }
        profileStr.withCString { profilePtr in
            relaysStr.withCString { relaysPtr in
                _ = nmp_app_chirp_create_new_account(raw, profilePtr, relaysPtr, mls, 1)
            }
        }
        return nil
    }

    /// Publish a kind:0 profile metadata event for the active account through
    /// the generated `GeneratedActionBuilders.publishProfile` bytes (M14-1 /
    /// #2145). Swift supplies profile fields only; Rust builds the kind:0 event,
    /// `created_at` stamp, and signature. Empty fields are OMITTED (so a blank
    /// `about` / `picture` is not written as an empty metadata key — preserving
    /// the prior behaviour). PR-A: returns the synchronous dispatch result so
    /// the caller can drive a spinner keyed on the correlation_id (or surface
    /// the error envelope to the user).
    @discardableResult
    func publishProfile(name: String, about: String, picture: String) -> DispatchResult {
        let id = UUID().uuidString
        var fields: [(String, String)] = []
        if !name.isEmpty { fields.append(("name", name)) }
        if !about.isEmpty { fields.append(("about", about)) }
        if !picture.isEmpty { fields.append(("picture", picture)) }
        return dispatchBytes(GeneratedActionBuilders.publishProfile(correlationId: id, fields: fields))
    }

    func switchActive(identityID: String) {
        identityID.withCString { nmp_app_switch_active(raw, $0) }
    }

    func removeAccount(identityID: String) {
        identityID.withCString { nmp_app_remove_account(raw, $0) }
    }
}
