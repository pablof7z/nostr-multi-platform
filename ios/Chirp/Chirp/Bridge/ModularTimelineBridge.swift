import Foundation

// ─────────────────────────────────────────────────────────────────────────
// T146 — Modular-timeline FFI bridge.
//
// Extracted from `KernelBridge.swift` to keep that file under the
// AGENTS.md 500-LOC hard cap. Public surface:
//
//   • `KernelHandle.registerChirpProjection()` — invoked from `init` to
//     plug `nmp_app_chirp_register` into the kernel event observer slot.
//     Idempotent: safe to call when `chirpHandle` is nil OR already set.
//   • `KernelHandle.unregisterChirpProjectionIfNeeded()` — drops the
//     projection before `nmp_app_free` (FFI contract).
//   • `KernelHandle.chirpSnapshot()` — JSON snapshot decoded into
//     `ChirpTimelineSnapshot`. Returns `.empty` on any failure (D6).
//   • `KernelHandle.reregisterChirpProjection()` — used by
//     `KernelModel.resetAndRestart()` so the projection's grouper state
//     is dropped on account switch / reset.
//
// All paths log via the shared `kbLog` defined in `KernelBridge.swift`.
// ─────────────────────────────────────────────────────────────────────────

extension KernelHandle {
    /// Register the Chirp modular timeline projection on the kernel event
    /// observer slot. Viewer pubkey is `nil` on cold boot — `signInNsec`
    /// etc. retarget the projection once an account becomes active
    /// (`Spec.viewer` is currently only used for future personalization
    /// keys; the grouper accepts every kind:1 the kernel ingests
    /// regardless). Idempotent.
    func registerChirpProjection() {
        chirpHandle = nmp_app_chirp_register(raw, nil)
        if chirpHandle == nil {
            kbLog.error("nmp_app_chirp_register returned NULL — projection unavailable")
        }
    }

    /// Drop the projection's observer registration if one exists. Called
    /// from `deinit` before `nmp_app_free`. Idempotent (no-op when
    /// `chirpHandle == nil`).
    func unregisterChirpProjectionIfNeeded() {
        if let handle = chirpHandle {
            nmp_app_chirp_unregister(handle)
            chirpHandle = nil
        }
    }

    /// Decode the current modular timeline snapshot. Returns
    /// `ChirpTimelineSnapshot.empty` when the projection handle is unset
    /// (registration failed) or when JSON parse fails (D6 — never throws
    /// across the bridge; logs and continues).
    func chirpSnapshot() -> ChirpTimelineSnapshot {
        guard let handle = chirpHandle else { return .empty }
        guard let ptr = nmp_app_chirp_snapshot(handle) else { return .empty }
        defer { nmp_app_chirp_snapshot_free(ptr) }
        let payload = String(cString: ptr)
        guard let data = payload.data(using: .utf8) else { return .empty }
        do {
            return try JSONDecoder().decode(ChirpTimelineSnapshot.self, from: data)
        } catch {
            kbLog.error("chirpSnapshot decode failed: \(error.localizedDescription)")
            return .empty
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
        _ = dispatchRawAction(namespace: "nmp.nip01.visible_note_relations", bodyJson: json)
    }
}
