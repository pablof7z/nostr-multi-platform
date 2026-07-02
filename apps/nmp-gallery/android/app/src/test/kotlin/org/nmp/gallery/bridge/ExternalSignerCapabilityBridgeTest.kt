package org.nmp.gallery.bridge

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.nmp.gallery.registry.ExternalSignerOutcome
import org.nmp.gallery.registry.ExternalSignerRequest
import org.nmp.gallery.registry.ExternalSignerResponse
import org.nmp.gallery.registry.Nip55Permission
import org.nmp.gallery.registry.buildAmberPermissionsJsonInternal
import org.nmp.gallery.registry.selectAmberResultValue
import org.nmp.gallery.registry.shouldUseContentResolver

/**
 * ADR-0072 Stage 2 — D7 contract tests for [ExternalSignerCapabilityBridge].
 *
 * The bridge must:
 *  1. Deserialise `ExternalSignerRequest` JSON produced by Rust without loss.
 *  2. Serialise `ExternalSignerResponse` JSON in a shape Rust can parse.
 *  3. Select ContentResolver when `granted_permissions` contains the method
 *     AND `signer_package` is known AND `force_interactive` is false.
 *  4. Fall through to the Intent path in every other case.
 *  5. Correctly map Rust method tags to NIP-55 Intent URI components.
 *
 * These are pure Kotlin unit tests — no Activity, no PackageManager, no
 * ContentProvider. The bridge's OS seams are validated separately in the
 * Stage-4 emulator E2E (ADR-0072 D7).
 */
class ExternalSignerCapabilityBridgeTest {

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
        classDiscriminator = "kind"
    }

    // ── ExternalSignerRequest round-trip ──────────────────────────────────

    @Test
    fun signEventRequestDeserialises() {
        val raw = """
            {
                "correlation_id": "abc123",
                "method": "sign_event",
                "payload": "{\"kind\":1,\"content\":\"hello\"}",
                "current_user": "deadbeef",
                "counterparty": null,
                "permissions": [],
                "granted_permissions": [
                    {"kind": "sign_event:1"}
                ],
                "signer_package": "com.greenart7c3.nostrsigner",
                "force_interactive": false
            }
        """.trimIndent()

        val req = json.decodeFromString<ExternalSignerRequest>(raw)
        assertEquals("abc123", req.correlationId)
        assertEquals("sign_event", req.method)
        assertEquals("deadbeef", req.currentUser)
        assertNull(req.counterparty)
        assertEquals("com.greenart7c3.nostrsigner", req.signerPackage)
        assertTrue(req.permissions.isEmpty())
        assertEquals(1, req.grantedPermissions.size)
    }

    @Test
    fun getPublicKeyRequestWithPermissionsDeserialises() {
        val raw = """
            {
                "correlation_id": "perm-req-1",
                "method": "get_public_key",
                "payload": "",
                "current_user": null,
                "permissions": [
                    {"kind": "sign_event:1"},
                    {"kind": "nip44_encrypt"},
                    {"kind": "nip44_decrypt"}
                ],
                "signer_package": null,
                "force_interactive": false
            }
        """.trimIndent()

        val req = json.decodeFromString<ExternalSignerRequest>(raw)
        assertEquals("get_public_key", req.method)
        assertNull(req.currentUser)
        assertNull(req.signerPackage)
        assertEquals(3, req.permissions.size)
        assertEquals("sign_event:1", req.permissions[0].kind)
        assertEquals("nip44_encrypt", req.permissions[1].kind)
        assertEquals("nip44_decrypt", req.permissions[2].kind)
    }

    @Test
    fun forceInteractiveDefaultsFalse() {
        val raw = """{"correlation_id":"x","method":"sign_event","payload":"{}"}"""
        val req = json.decodeFromString<ExternalSignerRequest>(raw)
        assertEquals(false, req.forceInteractive)
    }

    // ── ExternalSignerResponse round-trip ─────────────────────────────────

    @Test
    fun okResponseSerialises() {
        val resp = ExternalSignerResponse(
            correlationId = "abc123",
            outcome = ExternalSignerOutcome.Ok(result = "signedEventJsonHere"),
            signerPackage = null,
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), resp)
        assertTrue(encoded.contains("\"ok\"") || encoded.contains("\"kind\":\"ok\""))
        assertTrue(encoded.contains("abc123"))
        assertTrue(encoded.contains("signedEventJsonHere"))
    }

    @Test
    fun rejectedResponseSerialises() {
        val resp = ExternalSignerResponse(
            correlationId = "abc123",
            outcome = ExternalSignerOutcome.Rejected(reason = "user cancelled"),
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), resp)
        assertTrue(encoded.contains("user cancelled"))
    }

    @Test
    fun unavailableResponseSerialises() {
        val resp = ExternalSignerResponse(
            correlationId = "no-pkg",
            outcome = ExternalSignerOutcome.Unavailable(reason = "signer not installed"),
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), resp)
        assertTrue(encoded.contains("signer not installed"))
    }

    @Test
    fun signerPackagePopulatedOnGetPublicKeyReply() {
        val resp = ExternalSignerResponse(
            correlationId = "gpk-1",
            outcome = ExternalSignerOutcome.Ok(result = "aabbccdd"),
            signerPackage = "com.greenart7c3.nostrsigner",
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), resp)
        assertTrue(encoded.contains("com.greenart7c3.nostrsigner"))
    }

    // ── Transport-path selection logic ────────────────────────────────────
    //
    // These tests exercise the PRODUCTION `shouldUseContentResolver`
    // predicate — the exact function `ExternalSignerCapabilityBridge.handle()`
    // branches on (extracted as an internal pure function so no test-side
    // mirror exists). Rule (D7, mechanical):
    //   ContentResolver iff NOT forceInteractive AND signerPackage != null
    //   AND the method's permission kind is in the request's batch.

    @Test
    fun contentResolverSelectedWhenPermissionGrantedAndNotForced() {
        val req = ExternalSignerRequest(
            correlationId = "cr-1",
            method = "nip44_encrypt",
            payload = "plaintext",
            currentUser = "pubkeyhex",
            signerPackage = "com.greenart7c3.nostrsigner",
            grantedPermissions = listOf(Nip55Permission("nip44_encrypt")),
            forceInteractive = false,
        )
        assertTrue(shouldUseContentResolver(req))
    }

    @Test
    fun intentSelectedWhenForceInteractive() {
        val req = ExternalSignerRequest(
            correlationId = "intent-1",
            method = "nip44_encrypt",
            payload = "plaintext",
            signerPackage = "com.greenart7c3.nostrsigner",
            grantedPermissions = listOf(Nip55Permission("nip44_encrypt")),
            forceInteractive = true,
        )
        assertTrue(!shouldUseContentResolver(req))
    }

    @Test
    fun intentSelectedWhenSignerPackageUnknown() {
        val req = ExternalSignerRequest(
            correlationId = "intent-2",
            method = "nip44_encrypt",
            payload = "plaintext",
            signerPackage = null, // unknown
            grantedPermissions = listOf(Nip55Permission("nip44_encrypt")),
            forceInteractive = false,
        )
        assertTrue(!shouldUseContentResolver(req))
    }

    @Test
    fun intentSelectedWhenPermissionNotGranted() {
        val req = ExternalSignerRequest(
            correlationId = "intent-3",
            method = "nip44_decrypt",
            payload = "ciphertext",
            signerPackage = "com.greenart7c3.nostrsigner",
            grantedPermissions = emptyList(), // no permissions granted
            forceInteractive = false,
        )
        assertTrue(!shouldUseContentResolver(req))
    }

    @Test
    fun intentSelectedWhenPermissionOnlyRequestedButNotGranted() {
        val req = ExternalSignerRequest(
            correlationId = "intent-requested-only",
            method = "nip44_encrypt",
            payload = "plaintext",
            signerPackage = "com.greenart7c3.nostrsigner",
            permissions = listOf(Nip55Permission("nip44_encrypt")),
            grantedPermissions = emptyList(),
            forceInteractive = false,
        )
        assertTrue(!shouldUseContentResolver(req))
    }

    @Test
    fun contentResolverSelectedForSignEventWhenKindPermissionGranted() {
        // sign_event:1 grants sign_event for kind:1. The prefix-match should
        // recognise "sign_event:" as the permission kind for "sign_event".
        val req = ExternalSignerRequest(
            correlationId = "cr-sign-1",
            method = "sign_event",
            payload = "{\"kind\":1}",
            currentUser = "pubkeyhex",
            signerPackage = "com.greenart7c3.nostrsigner",
            grantedPermissions = listOf(Nip55Permission("sign_event:1")),
            forceInteractive = false,
        )
        assertTrue(shouldUseContentResolver(req))
    }

    // ── buildAmberPermissionsJsonInternal — Stage-4 regression ───────────
    //
    // Before the Stage-4 fix, dispatchIntent appended permissions to the URI
    // query string as `[{"kind":"sign_event:1"}]` (our internal format). Amber
    // expects Intent extras with `[{"type":"sign_event","kind":1}]`.
    // These tests pin the corrected encoding.

    @Test
    fun buildAmberPermissionsJson_signEvent_kindSplit() {
        // "sign_event:1" → {"type":"sign_event","kind":1}
        val result = buildAmberPermissionsJsonInternal(listOf(Nip55Permission("sign_event:1")))
        assertEquals("""[{"type":"sign_event","kind":1}]""", result)
    }

    @Test
    fun buildAmberPermissionsJson_noColonMethod() {
        // "nip44_encrypt" → {"type":"nip44_encrypt"}
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

    @Test
    fun buildAmberPermissionsJson_getPublicKey() {
        // get_public_key has no colon variant — just the method name
        val result = buildAmberPermissionsJsonInternal(listOf(Nip55Permission("get_public_key")))
        assertEquals("""[{"type":"get_public_key"}]""", result)
    }

    // ── selectAmberResultValue — Stage-4 sign_event regression ───────────
    //
    // Amber's RESULT_OK reply for `sign_event` carries the signature hex in
    // `result` and the FULL signed-event JSON in `event`. Rust verifies the
    // complete event (id + schnorr sig), so the bridge must hand back the
    // `event` extra for sign_event and `result` for everything else.

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
        assertEquals(
            "sighex",
            selectAmberResultValue("sign_event", eventExtra = null, resultExtra = "sighex"),
        )
    }

    @Test
    fun getPublicKeyUsesResultExtra() {
        // Amber sets event == result for get_public_key, but the contract is
        // `result`; the bridge must not depend on the duplication.
        assertEquals(
            "pubkeyhex",
            selectAmberResultValue("get_public_key", eventExtra = "pubkeyhex", resultExtra = "pubkeyhex"),
        )
        assertEquals(
            "pubkeyhex",
            selectAmberResultValue("get_public_key", eventExtra = null, resultExtra = "pubkeyhex"),
        )
    }

    @Test
    fun encryptUsesResultExtra() {
        assertEquals(
            "ciphertext",
            selectAmberResultValue("nip44_encrypt", eventExtra = null, resultExtra = "ciphertext"),
        )
    }

    @Test
    fun missingExtrasYieldNull() {
        assertNull(selectAmberResultValue("sign_event", eventExtra = null, resultExtra = null))
        assertNull(selectAmberResultValue("get_public_key", eventExtra = "x", resultExtra = null))
    }

    // ── KNOWN_NOSTR_SIGNERS contract ──────────────────────────────────────

    @Test
    fun amberIsInKnownSigners() {
        val amber = org.nmp.gallery.registry.KNOWN_NOSTR_SIGNERS.firstOrNull {
            it.intentScheme == "nostrsigner"
        }
        assertNotNull("Amber must be in KNOWN_NOSTR_SIGNERS", amber)
        assertEquals("com.greenart7c3.nostrsigner", amber!!.contentAuthority)
        // packageName must be set explicitly — the signer_package wire field
        // carries the APK identifier, not the ContentProvider authority.
        assertEquals("com.greenart7c3.nostrsigner", amber.packageName)
    }

    @Test
    fun primalIsInKnownSigners() {
        val primal = org.nmp.gallery.registry.KNOWN_NOSTR_SIGNERS.firstOrNull {
            it.intentScheme == "primal"
        }
        assertNotNull("Primal must be in KNOWN_NOSTR_SIGNERS", primal)
        // Primal uses the Intent-only path (no ContentProvider fast-path).
        assertNull(primal!!.contentAuthority)
        // The APK package name (net.primal.android) is distinct from the
        // intent scheme and must be set explicitly for signer_package routing.
        assertEquals("net.primal.android", primal.packageName)
    }
}
