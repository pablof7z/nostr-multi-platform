import Foundation
import os.log

private let kbLog = Logger(subsystem: "org.nmp.gallery", category: "GalleryKernelBridge")

/// Thin Swift wrapper around the gallery's app-owned UniFFI facade.
final class GalleryKernelHandle {
    private let app: GalleryApp
    private var retainedUpdateSink: GalleryUpdateSinkAdapter?

    init() {
        app = GalleryApp()
        configureStoragePath()
    }

    deinit {
        clearUpdateCallback()
        app.shutdown()
    }

    private func configureStoragePath() {
        guard let base = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first else {
            return
        }
        let directory = base.appendingPathComponent("NmpGallery", isDirectory: true)
        do {
            try FileManager.default.createDirectory(
                at: directory,
                withIntermediateDirectories: true)
            app.setStoragePath(path: directory.path)
        } catch {
            kbLog.error("failed to create NmpGallery storage dir: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Register the push callback that delivers each FlatBuffers update frame.
    /// The closure is invoked from the kernel actor thread on every emit tick.
    /// Callers are responsible for thread-hopping if they need main-actor
    /// isolation.
    func listen(_ handler: @escaping @Sendable (Data) -> Void) {
        clearUpdateCallback()
        let sink = GalleryUpdateSinkAdapter(handler: handler)
        retainedUpdateSink = sink
        app.setUpdateSink(sink: sink)
    }

    private func clearUpdateCallback() {
        guard retainedUpdateSink != nil else { return }
        app.setUpdateSink(sink: nil)
        retainedUpdateSink = nil
    }

    /// Configure the kernel and start the actor thread.
    func start() {
        app.start(visibleLimit: 80, emitHz: 4)
    }

    func stop() {
        app.stop()
    }

    // MARK: - Profile resolution

    func resolveProfileRef(pubkey: String, consumerID: String) {
        app.resolveProfileRef(key: pubkey, consumerId: consumerID)
    }

    func resolveProfileCard(pubkey: String, consumerID: String) {
        app.resolveProfileCard(key: pubkey, consumerId: consumerID)
    }

    func releaseProfileRef(pubkey: String, consumerID: String) {
        app.releaseProfileRef(key: pubkey, consumerId: consumerID)
    }

    // MARK: - Event-ref resolve / release

    func resolveEventRef(uri: String, consumerID: String, force: Bool = false) {
        guard let eventRef = app.eventRefFromUri(uri: uri) else { return }
        if force {
            app.resolveEventEmbedLiveWithMetadata(
                key: eventRef.key,
                consumerId: consumerID,
                metadata: eventRef.metadata)
        } else {
            app.resolveEventEmbedWithMetadata(
                key: eventRef.key,
                consumerId: consumerID,
                metadata: eventRef.metadata)
        }
    }

    func releaseEventRef(uri: String, consumerID: String) {
        guard let eventRef = app.eventRefFromUri(uri: uri) else { return }
        app.releaseEventRef(key: eventRef.key, consumerId: consumerID)
    }

    // MARK: - Relay seeding

    func addRelay(url: String, role: String) {
        app.addRelay(url: url, role: role)
    }

    // MARK: - Showcase sign-in

    func signInNsec(_ secret: String) {
        app.signinNsec(secret: secret, makeActive: true)
    }

    func snapshotJSONData(from data: Data) -> Data? {
        guard let json = app.decodeSnapshotJson(frame: data) else {
            kbLog.error("gallery typed snapshot decode failed")
            return nil
        }
        return Data(json.utf8)
    }
}

private final class GalleryUpdateSinkAdapter: GalleryUpdateSink, @unchecked Sendable {
    let handler: @Sendable (Data) -> Void

    init(handler: @escaping @Sendable (Data) -> Void) {
        self.handler = handler
    }

    func onUpdate(frame: Data) {
        handler(frame)
    }
}
