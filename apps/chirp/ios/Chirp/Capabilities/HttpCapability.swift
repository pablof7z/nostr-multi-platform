import Foundation

// MARK: - HTTP capability
//
// iOS half of the kernel-side `HttpCapability`
// (`crates/nmp-core/src/substrate/http.rs`, namespace `nmp.http.capability`).
//
// The kernel never makes HTTP calls directly; it routes them through the
// capability socket so the platform supplies the transport. This is the
// `URLSession` implementation, the second capability after `KeychainCapability`.
//
// Doctrine (docs/product-spec/doctrine.md):
//   D6 — errors never cross the boundary as exceptions. A DNS/TLS/timeout
//        failure, a malformed request, or the capability being stopped all
//        return a populated `HttpResult.error` inside the envelope; this type
//        never `throw`s across `handle(_:)` / `handleJSON(_:)`.
//   D7 — a capability reports and executes; it never decides policy. It
//        performs the exact GET/POST the kernel asks for and reports the
//        result. *Which* URL to call and *what* to do with the bolt11 invoice
//        an LNURL-pay round-trip yields are NIP-57 (kernel) decisions.
//
// SYNCHRONOUS SOCKET — the capability socket
// (`crates/nmp-core/src/capability_socket.rs`) is synchronous: Rust calls the
// native callback and blocks the actor thread until it returns. So this
// implementation must block too — it drives `URLSession` and waits on a
// `DispatchSemaphore`. For a rare user-triggered action like a NIP-57 zap
// (~500ms per HTTP call) this is an acceptable MVP trade-off; see
// `docs/decisions/0023-http-capability-synchronous-socket.md`. The blocking
// wait is safe because Rust invokes this callback from the actor thread, never
// the main thread, and `URLSession` completion handlers run on the session's
// own delegate queue (a background `OperationQueue`), not the calling thread —
// so `semaphore.wait()` here can never deadlock against the completion.

// MARK: - HTTP payload / result vocabulary

/// HTTP verb. Mirrors the Rust `HttpMethod` (`#[serde(rename_all =
/// "UPPERCASE")]` — wire values `"GET"` / `"POST"`).
enum HttpMethod: String, Decodable {
    case get = "GET"
    case post = "POST"
}

/// Capability-private request payload — the decoded `payload_json`.
/// Mirrors the Rust `HttpRequest`.
struct HttpRequest: Decodable {
    let method: HttpMethod
    let url: String
    /// Header `[name, value]` pairs. Absent from the wire ⇒ empty.
    let headers: [[String]]
    /// Request body — present for POST, absent for GET.
    let body: String?

    enum CodingKeys: String, CodingKey {
        case method
        case url
        case headers
        case body
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        method = try c.decode(HttpMethod.self, forKey: .method)
        url = try c.decode(String.self, forKey: .url)
        headers = try c.decodeIfPresent([[String]].self, forKey: .headers) ?? []
        body = try c.decodeIfPresent(String.self, forKey: .body)
    }
}

/// Capability-private result payload — the encoded `result_json`. Mirrors the
/// Rust `HttpResult` (`#[serde(tag = "status", rename_all = "snake_case")]`):
///   `{"status":"ok","status_code":200,"body":"…"}`
///   `{"status":"error","message":"…"}`
///
/// There is no error *exception*: a transport failure is data (`status ==
/// "error"`), satisfying D6.
enum HttpResult: Encodable {
    /// Transport succeeded — `statusCode` is the raw HTTP status (a 200 and a
    /// 404 are both `ok`; interpreting it is the caller's policy, D7).
    case ok(statusCode: UInt16, body: String)
    /// Transport-level failure (DNS, TLS, timeout, malformed request, …).
    case error(message: String)

    enum CodingKeys: String, CodingKey {
        case status
        case statusCode = "status_code"
        case body
        case message
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case let .ok(statusCode, body):
            try c.encode("ok", forKey: .status)
            try c.encode(statusCode, forKey: .statusCode)
            try c.encode(body, forKey: .body)
        case let .error(message):
            try c.encode("error", forKey: .status)
            try c.encode(message, forKey: .message)
        }
    }
}

// MARK: - URLSession-backed capability

/// `URLSession` implementation of the HTTP capability.
///
/// Performs the GET/POST the kernel asks for and reports the raw result. The
/// call blocks the calling (actor) thread until the HTTP round-trip completes —
/// see the synchronous-socket note at the top of this file.
final class HttpCapability {
    static let namespace = "nmp.http.capability"

    /// Wall-clock ceiling for a single HTTP call. An LNURL endpoint that never
    /// answers must not stall the actor thread forever — on expiry the request
    /// is reported as a timeout `HttpResult.error` (D6).
    private let timeout: TimeInterval

    /// Dedicated session so completions run on a private background
    /// `OperationQueue` (never the main queue) — the `semaphore.wait()` below
    /// can therefore never deadlock against the completion handler.
    private let session: URLSession

    private var started = false

    init(timeout: TimeInterval = 20) {
        self.timeout = timeout
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = timeout
        config.waitsForConnectivity = false
        let queue = OperationQueue()
        queue.name = "io.f7z.chirp.HttpCapability"
        self.session = URLSession(configuration: config, delegate: nil, delegateQueue: queue)
    }

    // MARK: Lifecycle (idempotent)

    /// Idempotent. Marks the capability active so requests are served.
    func start() {
        started = true
    }

    /// Idempotent. Marks the handler inactive so late requests are rejected as
    /// data (`HttpResult.error`), not crashes. Does not cancel in-flight calls.
    func stop() {
        started = false
    }

    var isStarted: Bool { started }

    // MARK: Envelope handling (never throws — D6)

    /// Decode → execute → encode. Any failure (malformed request, capability
    /// stopped, transport error) is returned inside the envelope's
    /// `result_json`, never raised.
    func handle(_ request: CapabilityRequest) -> CapabilityEnvelope {
        let result = process(request)
        let resultJSON = Self.encode(result)
            ?? Self.encode(HttpResult.error(message: "result-encode-failed"))
            ?? "{\"status\":\"error\",\"message\":\"result-encode-failed\"}"
        return CapabilityEnvelope(
            namespace: Self.namespace,
            correlationID: request.correlationID,
            resultJSON: resultJSON)
    }

    /// Convenience entry point for FFI bridges that hand us the raw kernel
    /// `CapabilityRequest` JSON and want raw `CapabilityEnvelope` JSON back.
    /// Honors D6 end to end: malformed input yields an error envelope string.
    func handleJSON(_ requestJSON: String) -> String {
        guard
            let data = requestJSON.data(using: .utf8),
            let request = try? JSONDecoder().decode(CapabilityRequest.self, from: data)
        else {
            let env = CapabilityEnvelope(
                namespace: Self.namespace,
                correlationID: "",
                resultJSON: Self.encode(HttpResult.error(message: "malformed-request"))
                    ?? "{\"status\":\"error\",\"message\":\"malformed-request\"}")
            return Self.encode(env) ?? "{}"
        }
        return Self.encode(handle(request)) ?? "{}"
    }

    // MARK: - Internals

    private func process(_ request: CapabilityRequest) -> HttpResult {
        guard started else { return .error(message: "capability-stopped") }
        guard
            let payload = request.payloadJSON.data(using: .utf8),
            let httpRequest = try? JSONDecoder().decode(HttpRequest.self, from: payload)
        else {
            return .error(message: "malformed-payload")
        }
        guard let url = URL(string: httpRequest.url) else {
            return .error(message: "invalid-url")
        }

        var urlRequest = URLRequest(url: url)
        urlRequest.httpMethod = httpRequest.method.rawValue
        urlRequest.timeoutInterval = timeout
        for pair in httpRequest.headers where pair.count == 2 {
            urlRequest.setValue(pair[1], forHTTPHeaderField: pair[0])
        }
        if let body = httpRequest.body {
            urlRequest.httpBody = body.data(using: .utf8)
        }

        return perform(urlRequest)
    }

    /// Lock-guarded one-shot result box. The completion handler (running on the
    /// session's background queue) writes once; `perform` reads once after the
    /// semaphore wait. The `NSLock` makes the cross-thread handoff explicit so
    /// the Swift 6 concurrency checker is satisfied — a bare captured `var`
    /// would be flagged as a data race even though the semaphore orders it.
    private final class ResultBox: @unchecked Sendable {
        private let lock = NSLock()
        private var value: HttpResult = .error(message: "no-response")

        func set(_ result: HttpResult) {
            lock.lock()
            value = result
            lock.unlock()
        }

        func get() -> HttpResult {
            lock.lock()
            defer { lock.unlock() }
            return value
        }
    }

    /// Drive `urlRequest` to completion synchronously. Blocks the calling
    /// thread on a `DispatchSemaphore` until the `URLSession` completion
    /// handler fires on the session's private background queue — safe because
    /// the two are different threads (see the file-level note).
    private func perform(_ urlRequest: URLRequest) -> HttpResult {
        let semaphore = DispatchSemaphore(value: 0)
        let box = ResultBox()

        let task = session.dataTask(with: urlRequest) { data, response, error in
            defer { semaphore.signal() }
            if let error {
                box.set(.error(message: "transport: \(error.localizedDescription)"))
                return
            }
            guard let http = response as? HTTPURLResponse else {
                box.set(.error(message: "non-http-response"))
                return
            }
            let body = data.flatMap { String(data: $0, encoding: .utf8) } ?? ""
            box.set(.ok(statusCode: UInt16(clamping: http.statusCode), body: body))
        }
        task.resume()
        // A generous ceiling above `timeoutIntervalForRequest` so the session's
        // own timeout fires first; this `wait` deadline is the backstop.
        if semaphore.wait(timeout: .now() + timeout + 5) == .timedOut {
            task.cancel()
            return .error(message: "timeout")
        }
        return box.get()
    }

    private static func encode<T: Encodable>(_ value: T) -> String? {
        guard let data = try? JSONEncoder().encode(value) else { return nil }
        return String(data: data, encoding: .utf8)
    }
}
