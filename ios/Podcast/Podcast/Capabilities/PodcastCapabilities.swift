import Foundation

/// Capability injection point for the Podcast app.
///
/// The kernel grants the app a set of capability *sockets*; the app supplies
/// the platform implementation. This holder is the one place those
/// implementations are constructed and started, mirroring the thin-bridge
/// pattern in `Bridge/KernelBridge.swift`.
///
/// It owns the `KeychainCapability` (at-rest secret storage) and the
/// `HttpCapability` (host HTTP transport — covers LNURL-pay, LLM SSE at M5,
/// and TTS WebSocket at M8). Additional podcast-specific capabilities
/// (audio, download, STT, TTS, spotlight, CarPlay) are registered in later
/// milestones following the same dispatcher pattern below.
/// Rust decides when and what to call; Swift only executes the request and
/// reports the raw result (D7).
///
/// There is a single C capability callback (`nmp_app_set_capability_callback`);
/// it routes by the `namespace` field of the incoming `CapabilityRequest` —
/// see [`handleJSON(_:)`].
///
/// ## KernelBridge dependency (M0.B)
///
/// `PodcastCapabilities` is wired into `KernelBridge` via the capability
/// callback socket — the same pattern Chirp uses in its `KernelBridge.swift`
/// (`nmp_app_set_capability_callback` receives `capabilities.handleJSON`).
/// The M0.B agent owns the bridge side; this type is the implementation side.
/// Neither side references the other's internals: the boundary is the JSON
/// envelope contract (`CapabilityRequest` / `CapabilityEnvelope`).
final class PodcastCapabilities {
    let keyring: KeychainCapability
    let http: HttpCapability

    init(
        keyring: KeychainCapability = KeychainCapability(),
        http: HttpCapability = HttpCapability()
    ) {
        self.keyring = keyring
        self.http = http
    }

    /// Idempotent: start all owned capabilities. Safe to call on every app
    /// foreground.
    func start() {
        keyring.start()
        http.start()
    }

    /// Idempotent: mark capabilities inactive. Does not erase stored secrets.
    func stop() {
        keyring.stop()
        http.stop()
    }

    /// Single capability-callback entry point. Routes the raw kernel
    /// `CapabilityRequest` JSON to the capability owning its `namespace` and
    /// returns the raw `CapabilityEnvelope` JSON.
    ///
    /// D6: an unparseable request or an unknown namespace yields a populated
    /// error envelope string, never a thrown error and never `nil`.
    func handleJSON(_ requestJSON: String) -> String {
        guard
            let data = requestJSON.data(using: .utf8),
            let request = try? JSONDecoder().decode(CapabilityRequest.self, from: data)
        else {
            // Cannot even read the namespace — return a generic error envelope.
            let env = CapabilityEnvelope(
                namespace: "",
                correlationID: "",
                resultJSON: "{\"status\":\"error\",\"message\":\"malformed-request\"}")
            return Self.encode(env) ?? "{}"
        }

        switch request.namespace {
        case KeychainCapability.namespace:
            return keyring.handleJSON(requestJSON)
        case HttpCapability.namespace:
            return http.handleJSON(requestJSON)
        default:
            // D6 — an unknown namespace is data, not a crash. Echo the
            // correlation id so the issuing kernel module can still correlate.
            let env = CapabilityEnvelope(
                namespace: request.namespace,
                correlationID: request.correlationID,
                resultJSON: "{\"status\":\"error\",\"message\":\"unknown-namespace\"}")
            return Self.encode(env) ?? "{}"
        }
    }

    private static func encode<T: Encodable>(_ value: T) -> String? {
        guard let data = try? JSONEncoder().encode(value) else { return nil }
        return String(data: data, encoding: .utf8)
    }
}
