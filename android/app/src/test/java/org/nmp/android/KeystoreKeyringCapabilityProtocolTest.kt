package org.nmp.android

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Protocol-shape tests for the Android keyring capability.
 *
 * The Android Keystore APIs (`KeyGenParameterSpec`, `KeyStore.getInstance("AndroidKeyStore")`)
 * are not available in the plain JVM unit-test harness, so we cannot exercise
 * the actual encryption path here. Instrumented tests (androidTest) cover that.
 *
 * What we verify here:
 * 1. [KeystoreKeyringCapability.NAMESPACE] matches the Rust constant.
 * 2. The `KeyringResult` JSON shapes match the Rust wire vocabulary so a
 *    round-trip through the kernel's `KeyringIdentityWiring::decode_result`
 *    succeeds.
 * 3. The outer `CapabilityEnvelope` JSON shape matches what the Rust dispatcher
 *    expects (`namespace`, `correlation_id`, `result_json`).
 * 4. The `CapabilityRequest` JSON shape matches what the Rust `KeyringIdentityWiring`
 *    emits (round-trip verification of the request vocabulary).
 *
 * These cover the D6 contract (errors are data) and D7 (envelope transport only).
 */
class KeystoreKeyringCapabilityProtocolTest {

    private val json = Json { ignoreUnknownKeys = true }

    // ── Local mirror of wire types (avoids pulling private classes) ───────────

    @Serializable
    private data class CapabilityEnvelope(
        val namespace: String,
        @SerialName("correlation_id") val correlationId: String,
        @SerialName("result_json") val resultJson: String,
    )

    @Serializable
    private data class KeyringResult(
        val status: String,
        val secret: String? = null,
        @SerialName("os_status") val osStatus: Int? = null,
    )

    @Serializable
    private data class KeyringCapabilityRequest(
        val namespace: String,
        @SerialName("correlation_id") val correlationId: String,
        @SerialName("payload_json") val payloadJson: String,
    )

    // ── NAMESPACE constant ────────────────────────────────────────────────────

    @Test
    fun namespaceMatchesRustConstant() {
        // Matches `KeyringCapability::NAMESPACE` in `nmp-core/src/substrate/keyring.rs`.
        assertEquals("nmp.keyring.capability", KeystoreKeyringCapability.NAMESPACE)
    }

    // ── KeyringResult wire shapes ─────────────────────────────────────────────

    @Test
    fun okResultWithSecretSerializesCorrectly() {
        val result = json.encodeToString(KeyringResult(status = "ok", secret = "nsec1abc"))
        assert(result.contains("\"status\":\"ok\"")) { "status field wrong: $result" }
        assert(result.contains("\"secret\":\"nsec1abc\"")) { "secret field wrong: $result" }
        assert(!result.contains("os_status")) { "os_status must be absent: $result" }
    }

    @Test
    fun okResultWithoutSecretOmitsSecretField() {
        val result = json.encodeToString(KeyringResult(status = "ok"))
        assert(result.contains("\"status\":\"ok\"")) { "status field wrong: $result" }
        assert(!result.contains("secret")) { "secret field must be absent: $result" }
    }

    @Test
    fun notFoundResultSerializesCorrectly() {
        // Must match Rust `KeyringResult::not_found()` → `{"status":"not_found"}`.
        val result = json.encodeToString(KeyringResult(status = "not_found"))
        assertEquals("""{"status":"not_found"}""", result)
    }

    @Test
    fun errorResultWithCodeSerializesCorrectly() {
        // Must match Rust `KeyringResult::error(-1)` → `{"status":"error","os_status":-1}`.
        val result = json.encodeToString(KeyringResult(status = "error", osStatus = -1))
        assert(result.contains("\"status\":\"error\"")) { "status field wrong: $result" }
        assert(result.contains("\"os_status\":-1")) { "os_status wrong: $result" }
        assert(!result.contains("secret")) { "secret must be absent: $result" }
    }

    @Test
    fun keyringResultDecodesFromRustWireShape() {
        // Verify the local decoder handles Rust-emitted JSON correctly.
        val fromRust = """{"status":"ok","secret":"nsec1abc"}"""
        val decoded = json.decodeFromString<KeyringResult>(fromRust)
        assertEquals("ok", decoded.status)
        assertEquals("nsec1abc", decoded.secret)
        assertEquals(null, decoded.osStatus)
    }

    // ── CapabilityRequest / CapabilityEnvelope shapes ─────────────────────────

    @Test
    fun capabilityRequestRoundTrip() {
        // The kernel emits exactly this shape; verify we can round-trip it.
        val storePayload = """{"op":"store","account_id":"acct-1","secret":"nsec1abc"}"""
        val request = KeyringCapabilityRequest(
            namespace = "nmp.keyring.capability",
            correlationId = "corr-1",
            payloadJson = storePayload,
        )
        val encoded = json.encodeToString(request)
        val decoded = json.decodeFromString<KeyringCapabilityRequest>(encoded)
        assertEquals("nmp.keyring.capability", decoded.namespace)
        assertEquals("corr-1", decoded.correlationId)
        assert(decoded.payloadJson.contains("\"op\":\"store\"")) { decoded.payloadJson }
    }

    @Test
    fun capabilityEnvelopeRoundTrip() {
        // The handler must produce exactly this shape; verify the envelope fields.
        val resultJson = json.encodeToString(KeyringResult(status = "ok"))
        val envelope = CapabilityEnvelope(
            namespace = "nmp.keyring.capability",
            correlationId = "c1",
            resultJson = resultJson,
        )
        val encoded = json.encodeToString(envelope)
        val decoded = json.decodeFromString<CapabilityEnvelope>(encoded)
        assertEquals("nmp.keyring.capability", decoded.namespace)
        assertEquals("c1", decoded.correlationId)
        val result = json.decodeFromString<KeyringResult>(decoded.resultJson)
        assertEquals("ok", result.status)
    }

    @Test
    fun envelopeFieldNamesUseSnakeCase() {
        // The Rust dispatcher reads `correlation_id` and `result_json` verbatim.
        val envelope = CapabilityEnvelope(
            namespace = "nmp.keyring.capability",
            correlationId = "c2",
            resultJson = "{}",
        )
        val encoded = json.encodeToString(envelope)
        assert(encoded.contains("\"correlation_id\"")) { "must be snake_case: $encoded" }
        assert(encoded.contains("\"result_json\"")) { "must be snake_case: $encoded" }
    }
}
