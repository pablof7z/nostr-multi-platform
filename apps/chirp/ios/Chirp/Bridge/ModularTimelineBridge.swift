import Foundation

// ─────────────────────────────────────────────────────────────────────────
// Modular-timeline FFI bridge.
//
// Extracted from `KernelBridge.swift` to keep that file under the
// AGENTS.md 500-LOC hard cap. Public surface:
//
//   • `KernelHandle.registerChirpProjection()` — invoked from `init` to
//     plug `nmp_app_chirp_register` into declared observed projections.
//     Idempotent: safe to call when `chirpHandle` is nil OR already set.
//   • `KernelHandle.unregisterChirpProjectionIfNeeded()` — drops the
//     projection before `nmp_app_free` (FFI contract).
//   • `KernelHandle.loadOlderHomeFeed()` — high-level render intent that
//     advances the Rust-owned standard home-feed projection; Swift never
//     constructs cursors.
//   • `KernelHandle.reregisterChirpProjection()` — used by
//     `KernelModel.resetAndRestart()` so the projection's grouper state
//     is dropped on account switch / reset.
//
// All paths log via the shared `kbLog` defined in `KernelBridge.swift`.
// ─────────────────────────────────────────────────────────────────────────

extension KernelHandle {
    /// Register the Chirp modular timeline projection through declared
    /// observed projections. Viewer pubkey is `nil` on cold boot — `addSigner`
    /// etc. retarget the projection once an account becomes active
    /// (`Spec.viewer` is currently only used for future personalization
    /// keys; the grouper accepts every kind:1 the kernel ingests
    /// regardless). Idempotent.
    ///
    /// V-73 (D6 fix): the underlying `nmp_app_chirp_register` now returns
    /// an `NmpRegisterStatus` code and writes the handle through an
    /// out-parameter.  The host checks the status and surfaces an error log
    /// on any non-Ok result.  The common startup call passes a nil
    /// viewer_pubkey (always Ok); callers that pass a non-nil pubkey must
    /// ensure it is a valid 64-char hex string before calling.
    func registerChirpProjection() {
        var handle: UnsafeMutableRawPointer? = nil
        let status = nmp_app_chirp_register(raw, nil, &handle)
        if status == NmpRegisterStatus_Ok.rawValue, let h = handle {
            chirpHandle = h
        } else {
            chirpHandle = nil
            kbLog.error(
                "nmp_app_chirp_register failed — projection unavailable (status=\(status))"
            )
        }
    }

    /// Drop the projection registration if one exists. Called
    /// from `deinit` before `nmp_app_free`. Idempotent (no-op when
    /// `chirpHandle == nil`).
    func unregisterChirpProjectionIfNeeded() {
        if let handle = chirpHandle {
            nmp_app_chirp_unregister(handle)
            chirpHandle = nil
        }
    }

    func loadOlderHomeFeed() {
        "nmp.feed.home".withCString { key in
            nmp_app_load_older_feed(raw, key)
        }
    }

    /// Drop the current projection and register a fresh one. Called
    /// after `nmp_app_reset` (which clears the kernel's read-cache but
    /// cannot reach inside the projection's state). The new handle
    /// starts empty; the next batch of events repopulates it.
    func reregisterChirpProjection() {
        unregisterChirpProjectionIfNeeded()
        registerChirpProjection()
    }

    func claimVisibleNoteRelations(eventID: String) {
        dispatchVisibleNoteRelations(op: "claim", eventID: eventID)
    }

    func releaseVisibleNoteRelations(eventID: String) {
        dispatchVisibleNoteRelations(op: "release", eventID: eventID)
    }

    private func dispatchVisibleNoteRelations(op: String, eventID: String) {
        let body: [String: String] = [
            "op": op,
            "event_id": eventID,
            "consumer_id": "ios.visible-note:\(eventID)"
        ]
        guard
            JSONSerialization.isValidJSONObject(body),
            let data = try? JSONSerialization.data(withJSONObject: body),
            let json = String(data: data, encoding: .utf8)
        else { return }
        dispatchVisibleNoteRelations(bodyJson: json)
    }

    private func dispatchVisibleNoteRelations(bodyJson: String) {
        let namespace = "nmp.nip01.visible_note_relations"
        bodyJson.withCString { jsonPtr in
            namespace.withCString { nsPtr in
                if let ptr = nmp_app_chirp_dispatch_action_bytes(raw, nsPtr, jsonPtr) {
                    nmp_free_string(ptr)
                }
            }
        }
    }
}
