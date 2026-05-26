import Darwin
import Foundation
import Observation

/// Thin Swift wrapper over `libnmp_app_notes.a`. Every method delegates to
/// generic substrate symbols already exported by `nmp-core` +
/// `nmp-ffi` — no Notes-specific protocol logic lives here.
@Observable
final class NotesBridge {
    private let raw: UnsafeMutableRawPointer
    var isSignedIn = false
    var notes: [NoteModel] = []
    private var observerID: UInt64 = 0
    private let observerBox = ObserverBox()

    init() {
        raw = nmp_app_new()
        observerBox.bridge = self
        Self.configureStorage(for: raw)
        nmp_signer_broker_init(raw)   // NIP-46 broker (bunker sign-in + nostrconnect URI)
        nmp_app_notes_init(raw)       // app-registration marker (no-op in the spike)
    }

    deinit {
        if observerID != 0 { nmp_app_unregister_raw_event_observer(raw, observerID) }
        nmp_app_stop(raw); nmp_app_free(raw)
    }

    /// Boot the kernel actor and attach the kind:1 observer. Idempotent.
    func start() { nmp_app_start(raw, 0, 0, 0); registerNoteObserver() }
    func foreground() { nmp_app_lifecycle_foreground(raw) }
    func background() { nmp_app_lifecycle_background(raw) }
    func signInNsec(_ secret: String) {
        secret.withCString { nmp_app_signin_nsec(raw, $0) }; isSignedIn = true
    }
    func signInBunker(_ uri: String) {
        uri.withCString { nmp_app_signin_bunker(raw, $0) }; isSignedIn = true
    }
    /// Fresh `nostrconnect://` URI for QR-code NIP-46 sign-in.
    func generateBunkerURI() -> String? {
        guard let ptr = nmp_app_nostrconnect_uri(raw, nil, nil) else { return nil }
        defer { nmp_broker_free_string(ptr) }
        return String(cString: ptr)
    }

    /// Publish a kind:1 note through the single generic dispatch door.
    /// Body shape identical to Chirp's `KernelBridge.publishNote`, proving
    /// the action surface is generic, not Chirp-specific.
    func publishNote(_ content: String) {
        let body: [String: Any] = ["PublishNote": [
            "content": content, "reply_to_id": NSNull(), "target": "Auto",
        ]]
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let json = String(data: data, encoding: .utf8) else { return }
        json.withCString { jsonPtr in
            "nmp.publish".withCString { nsPtr in
                guard let env = nmp_app_dispatch_action(raw, nsPtr, jsonPtr) else { return }
                nmp_app_free_string(env)
            }
        }
    }

    private static func configureStorage(for raw: UnsafeMutableRawPointer) {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory,
                                                  in: .userDomainMask).first else { return }
        let dir = base.appendingPathComponent("NMPNotes", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        dir.path.withCString { nmp_app_set_storage_path(raw, $0) }
    }

    private func registerNoteObserver() {
        guard observerID == 0 else { return }
        let ctx = Unmanaged.passUnretained(observerBox).toOpaque()
        observerID = "[1]".withCString {
            nmp_app_register_raw_event_observer(raw, ctx, rawNoteObserverCallback, $0)
        }
    }

    /// Called from the C callback on the actor thread — hops to the main
    /// thread before mutating `notes` (SwiftUI thread-safety).
    fileprivate func ingestRawEventJSON(_ json: String) {
        guard let note = NoteModel.parse(json) else { return }
        DispatchQueue.main.async { [weak self] in
            guard let self, !self.notes.contains(where: { $0.id == note.id }) else { return }
            self.notes.insert(note, at: 0)
        }
    }
}

private final class ObserverBox { weak var bridge: NotesBridge? }

private let rawNoteObserverCallback: NmpRawEventObserverCallback = { context, jsonPtr in
    guard let context, let jsonPtr else { return }
    let box = Unmanaged<ObserverBox>.fromOpaque(context).takeUnretainedValue()
    box.bridge?.ingestRawEventJSON(String(cString: jsonPtr))
}
