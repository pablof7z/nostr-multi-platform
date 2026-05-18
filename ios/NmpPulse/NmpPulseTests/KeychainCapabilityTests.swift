import XCTest
@testable import NmpPulse

/// Round-trips the Keychain-backed keyring capability against the simulator's
/// real Keychain. Uses a per-run unique service so concurrent/repeat runs do
/// not collide.
final class KeychainCapabilityTests: XCTestCase {
    private var cap: KeychainCapability!
    private var accountID: String!

    override func setUp() {
        super.setUp()
        cap = KeychainCapability(service: "test.nmppulse.keyring.\(UUID().uuidString)")
        accountID = "acct-\(UUID().uuidString)"
        cap.start()
    }

    override func tearDown() {
        _ = cap.handle(makeRequest(deletePayload(accountID)))
        cap.stop()
        cap = nil
        super.tearDown()
    }

    func testStartStopIsIdempotent() {
        cap.start()
        cap.start()
        XCTAssertTrue(cap.isStarted)
        cap.stop()
        cap.stop()
        XCTAssertFalse(cap.isStarted)
        cap.start()
        XCTAssertTrue(cap.isStarted)
    }

    func testStoreThenRetrieveRoundTrip() {
        let secret = "nsec1exampleimportedsecret\(UUID().uuidString)"

        let storeEnv = cap.handle(makeRequest(storePayload(accountID, secret)))
        XCTAssertEqual(storeEnv.namespace, KeychainCapability.namespace)
        let storeResult = decodeResult(storeEnv)
        XCTAssertEqual(
            storeResult.status, "ok",
            "store failed; OSStatus=\(storeResult.osStatus.map(String.init) ?? "nil")")

        let retrieveEnv = cap.handle(makeRequest(retrievePayload(accountID)))
        let retrieveResult = decodeResult(retrieveEnv)
        XCTAssertEqual(retrieveResult.status, "ok")
        XCTAssertEqual(retrieveResult.secret, secret)
        // correlation_id round-trips unchanged.
        XCTAssertFalse(retrieveEnv.correlationID.isEmpty)
    }

    func testStoreOverwritesPriorValue() {
        _ = cap.handle(makeRequest(storePayload(accountID, "first")))
        _ = cap.handle(makeRequest(storePayload(accountID, "second")))
        let result = decodeResult(cap.handle(makeRequest(retrievePayload(accountID))))
        XCTAssertEqual(result.secret, "second")
    }

    func testRetrieveMissingReportsNotFound() {
        let result = decodeResult(cap.handle(makeRequest(retrievePayload("never-stored"))))
        XCTAssertEqual(result.status, "not_found")
        XCTAssertNil(result.secret)
    }

    func testDeleteIsIdempotentAndAffectsRetrieve() {
        _ = cap.handle(makeRequest(storePayload(accountID, "x")))
        XCTAssertEqual(decodeResult(cap.handle(makeRequest(deletePayload(accountID)))).status, "ok")
        // Deleting again is still "ok" (idempotent).
        XCTAssertEqual(decodeResult(cap.handle(makeRequest(deletePayload(accountID)))).status, "ok")
        XCTAssertEqual(
            decodeResult(cap.handle(makeRequest(retrievePayload(accountID)))).status, "not_found")
    }

    func testStoppedCapabilityReturnsErrorEnvelopeNotException() {
        cap.stop()
        let result = decodeResult(cap.handle(makeRequest(retrievePayload(accountID))))
        XCTAssertEqual(result.status, "error") // D6: failure is data, never a throw.
        cap.start()
    }

    func testMalformedPayloadReturnsErrorEnvelope() {
        let env = cap.handle(makeRequest("{ not json"))
        XCTAssertEqual(env.namespace, KeychainCapability.namespace)
        XCTAssertEqual(decodeResult(env).status, "error")
    }

    func testHandleJSONWithGarbageStillReturnsEnvelopeString() {
        let out = cap.handleJSON("definitely not a capability request")
        XCTAssertTrue(out.contains(KeychainCapability.namespace))
        XCTAssertTrue(out.contains("error"))
    }

    // MARK: - Helpers

    private func makeRequest(_ payloadJSON: String) -> CapabilityRequest {
        let wire = """
        {"namespace":"\(KeychainCapability.namespace)",\
        "correlation_id":"\(UUID().uuidString)",\
        "payload_json":\(jsonStringLiteral(payloadJSON))}
        """
        return try! JSONDecoder().decode(
            CapabilityRequest.self, from: wire.data(using: .utf8)!)
    }

    private func decodeResult(_ env: CapabilityEnvelope) -> KeyringDecoded {
        try! JSONDecoder().decode(
            KeyringDecoded.self, from: env.resultJSON.data(using: .utf8)!)
    }

    private struct KeyringDecoded: Decodable {
        let status: String
        let secret: String?
        let osStatus: Int32?

        enum CodingKeys: String, CodingKey {
            case status
            case secret
            case osStatus = "os_status"
        }
    }

    private func storePayload(_ id: String, _ secret: String) -> String {
        "{\"op\":\"store\",\"account_id\":\"\(id)\",\"secret\":\"\(secret)\"}"
    }
    private func retrievePayload(_ id: String) -> String {
        "{\"op\":\"retrieve\",\"account_id\":\"\(id)\"}"
    }
    private func deletePayload(_ id: String) -> String {
        "{\"op\":\"delete\",\"account_id\":\"\(id)\"}"
    }

    /// Encode an arbitrary string as a JSON string literal (escaped).
    private func jsonStringLiteral(_ s: String) -> String {
        let data = try! JSONEncoder().encode(s)
        return String(data: data, encoding: .utf8)!
    }
}
