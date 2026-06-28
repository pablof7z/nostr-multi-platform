package org.nmp.android

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * ADR-0048 Stage 2 — D7 contract tests for [ExternalSignerCapabilityBridge]
 * (Chirp vendored copy).
 *
 * Mirrors `ExternalSignerCapabilityBridgeTest` in the gallery test suite.
 * These tests run in `./gradlew :app:testDebugUnitTest -x cargoNdk`.
 */
class ExternalSignerCapabilityBridgeTest {

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
        classDiscriminator = "kind"
    }

    // ── Request round-trip ────────────────────────────────────────────────

    @Test
    fun signEventRequestDeserialises() {
        val raw = """
            {
                "correlation_id": "abc123",
                "method": "sign_event",
                "payload": "{\"kind\":1,\"content\":\"hello\"}",
                "current_user": "deadbeef",
                "signer_package": "com.greenart7c3.nostrsigner",
                "permissions": [],
                "granted_permissions": [
                    {"kind": "sign_event:1"}
                ],
                "force_interactive": false
            }
        """.trimIndent()
        val req = json.decodeFromString<ExternalSignerRequest>(raw)
        assertEquals("abc123", req.correlationId)
        assertEquals("sign_event", req.method)
        assertEquals("deadbeef", req.currentUser)
        assertEquals("com.greenart7c3.nostrsigner", req.signerPackage)
        assertEquals(1, req.grantedPermissions.size)
        assertFalse(req.forceInteractive)
    }

    @Test
    fun getPublicKeyWithPermissionsDeserialises() {
        val raw = """
            {
                "correlation_id": "gpk-1",
                "method": "get_public_key",
                "payload": "",
                "current_user": null,
                "permissions": [
                    {"kind": "sign_event:1"},
                    {"kind": "nip44_encrypt"}
                ],
                "signer_package": null,
                "force_interactive": false
            }
        """.trimIndent()
        val req = json.decodeFromString<ExternalSignerRequest>(raw)
        assertEquals("get_public_key", req.method)
        assertNull(req.currentUser)
        assertNull(req.signerPackage)
        assertEquals(2, req.permissions.size)
    }

    // ── Response round-trip ───────────────────────────────────────────────

    @Test
    fun okResponseRoundTrip() {
        val resp = ExternalSignerResponse(
            correlationId = "abc123",
            outcome = ExternalSignerOutcome.Ok(result = "signedJson"),
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), resp)
        val decoded = json.decodeFromString<ExternalSignerResponse>(encoded)
        assertEquals("abc123", decoded.correlationId)
        assertTrue(decoded.outcome is ExternalSignerOutcome.Ok)
        assertEquals("signedJson", (decoded.outcome as ExternalSignerOutcome.Ok).result)
    }

    @Test
    fun rejectedResponseRoundTrip() {
        val resp = ExternalSignerResponse(
            correlationId = "rej-1",
            outcome = ExternalSignerOutcome.Rejected(reason = "user cancelled"),
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), resp)
        val decoded = json.decodeFromString<ExternalSignerResponse>(encoded)
        assertTrue(decoded.outcome is ExternalSignerOutcome.Rejected)
    }

    @Test
    fun unavailableResponseRoundTrip() {
        val resp = ExternalSignerResponse(
            correlationId = "una-1",
            outcome = ExternalSignerOutcome.Unavailable(reason = "not installed"),
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), resp)
        val decoded = json.decodeFromString<ExternalSignerResponse>(encoded)
        assertTrue(decoded.outcome is ExternalSignerOutcome.Unavailable)
        assertEquals("not installed", (decoded.outcome as ExternalSignerOutcome.Unavailable).reason)
    }

    // ── Transport-path selection ──────────────────────────────────────────
    //
    // Exercises the PRODUCTION `shouldUseContentResolver` predicate — the
    // exact internal pure function `ExternalSignerCapabilityBridge.handle()`
    // branches on. No test-side mirror exists.

    @Test
    fun contentResolverSelectedWhenAllConditionsMet() {
        val req = ExternalSignerRequest(
            correlationId = "cr-1",
            method = "nip44_encrypt",
            payload = "plaintext",
            currentUser = "pubkey",
            signerPackage = "com.greenart7c3.nostrsigner",
            grantedPermissions = listOf(Nip55Permission("nip44_encrypt")),
            forceInteractive = false,
        )
        assertTrue(shouldUseContentResolver(req))
    }

    @Test
    fun intentSelectedWhenForceInteractive() {
        val req = ExternalSignerRequest(
            correlationId = "fi-1",
            method = "nip44_encrypt",
            payload = "plaintext",
            signerPackage = "com.greenart7c3.nostrsigner",
            grantedPermissions = listOf(Nip55Permission("nip44_encrypt")),
            forceInteractive = true,
        )
        assertFalse(shouldUseContentResolver(req))
    }

    @Test
    fun intentSelectedWhenNoSignerPackage() {
        val req = ExternalSignerRequest(
            correlationId = "nsp-1",
            method = "nip44_encrypt",
            payload = "plaintext",
            signerPackage = null,
            grantedPermissions = listOf(Nip55Permission("nip44_encrypt")),
            forceInteractive = false,
        )
        assertFalse(shouldUseContentResolver(req))
    }

    @Test
    fun intentSelectedWhenPermissionNotInGrantedSet() {
        val req = ExternalSignerRequest(
            correlationId = "np-1",
            method = "nip44_decrypt",
            payload = "ciphertext",
            signerPackage = "com.greenart7c3.nostrsigner",
            grantedPermissions = listOf(Nip55Permission("nip44_encrypt")), // only encrypt, not decrypt
            forceInteractive = false,
        )
        assertFalse(shouldUseContentResolver(req))
    }

    @Test
    fun intentSelectedWhenPermissionOnlyRequestedButNotGranted() {
        val req = ExternalSignerRequest(
            correlationId = "requested-only",
            method = "nip44_encrypt",
            payload = "plaintext",
            signerPackage = "com.greenart7c3.nostrsigner",
            permissions = listOf(Nip55Permission("nip44_encrypt")),
            grantedPermissions = emptyList(),
            forceInteractive = false,
        )
        assertFalse(shouldUseContentResolver(req))
    }

    // ── buildAmberPermissionsJsonInternal — Stage-4 regression ───────────

    @Test
    fun buildAmberPermissionsJson_signEvent_kindSplit() {
        val result = buildAmberPermissionsJsonInternal(listOf(Nip55Permission("sign_event:1")))
        assertEquals("""[{"type":"sign_event","kind":1}]""", result)
    }

    @Test
    fun buildAmberPermissionsJson_noColonMethod() {
        val result = buildAmberPermissionsJsonInternal(listOf(Nip55Permission("nip44_encrypt")))
        assertEquals("""[{"type":"nip44_encrypt"}]""", result)
    }

    @Test
    fun buildAmberPermissionsJson_multiplePermissions() {
        val perms = listOf(
            Nip55Permission("sign_event:1"),
            Nip55Permission("nip44_encrypt"),
            Nip55Permission("nip44_decrypt"),
        )
        val result = buildAmberPermissionsJsonInternal(perms)
        assertEquals(
            """[{"type":"sign_event","kind":1},{"type":"nip44_encrypt"},{"type":"nip44_decrypt"}]""",
            result,
        )
    }

    @Test
    fun buildAmberPermissionsJson_emptyList() {
        val result = buildAmberPermissionsJsonInternal(emptyList())
        assertEquals("[]", result)
    }

    // ── selectAmberResultValue — Stage-4 sign_event regression ───────────

    @Test
    fun signEventPrefersEventExtra() {
        val signedJson = """{"id":"abc","pubkey":"def","sig":"012"}"""
        assertEquals(
            signedJson,
            selectAmberResultValue("sign_event", eventExtra = signedJson, resultExtra = "sighex"),
        )
    }

    @Test
    fun signEventFallsBackToResultWhenEventBlank() {
        assertEquals(
            "sighex",
            selectAmberResultValue("sign_event", eventExtra = "", resultExtra = "sighex"),
        )
    }

    @Test
    fun getPublicKeyUsesResultExtra() {
        assertEquals(
            "pubkeyhex",
            selectAmberResultValue("get_public_key", eventExtra = null, resultExtra = "pubkeyhex"),
        )
    }

    @Test
    fun missingExtrasYieldNull() {
        assertNull(selectAmberResultValue("sign_event", eventExtra = null, resultExtra = null))
    }

    // ── KNOWN_NOSTR_SIGNERS ───────────────────────────────────────────────

    @Test
    fun amberIsInKnownSigners() {
        val amber = KNOWN_NOSTR_SIGNERS.firstOrNull { it.intentScheme == "nostrsigner" }
        assertNotNull("Amber must be in KNOWN_NOSTR_SIGNERS", amber)
        assertEquals("com.greenart7c3.nostrsigner", amber!!.contentAuthority)
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private fun assertFalse(value: Boolean) = assertTrue(!value)
}
