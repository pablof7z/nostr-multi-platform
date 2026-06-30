package org.nmp.gallery.bridge

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.nmp.gallery.registry.ExternalSignerOutcome
import org.nmp.gallery.registry.ExternalSignerRequest
import org.nmp.gallery.registry.ExternalSignerResponse
import org.nmp.gallery.registry.shouldUseContentResolver

/**
 * ADR-0048 Stage 2 — the Kotlin half of the sign-in loop, exercised against
 * the EXACT wire JSON the Rust driver produces and consumes.
 *
 * Loop coverage split (the full proof spans two suites):
 *  * **Rust** (`crates/nmp-ffi/src/external_signer.rs` tests) — drives the
 *    production `Nip55Driver` end to end: `signin` builds the
 *    `get_public_key` + permission-batch request, the raw reply resolves
 *    into `AddSigner { nip55, make_active }` + `signer_state: ready`, and a
 *    subsequent `sign` round-trips with full id+sig verification.
 *  * **Kotlin** (this file) — proves the production bridge types and the
 *    production transport-selection predicate are wire-compatible with the
 *    Rust ends of that loop: the request Rust built decodes losslessly,
 *    selection is mechanical, and the response Kotlin encodes is byte-shape
 *    compatible with `ExternalSignerResponse` on the Rust side.
 *
 * The remaining seam — the real Intent / ContentResolver OS round-trip — is
 * Android-runtime territory and is covered by the Stage-4 emulator E2E
 * against a real Amber APK (ADR-0048 D7: emulator E2E is the acceptance
 * oracle; the unit layers are the merge gates).
 */
class ExternalSignerLoopContractTest {

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
        classDiscriminator = "kind"
    }

    /**
     * Verbatim shape of the first-connect request `Nip55Connect::new` builds
     * for the NmpGallery composition root (serde: snake_case method tags,
     * permission batch, optional package).
     *
     * Issue #2523 / crate-boundaries.md §9 — the permission batch is no
     * longer a framework default baked into `nmp-signers`; it is the
     * gallery's own product decision (`gallery_nip55_permissions()` in
     * `apps/nmp-gallery/crates/nmp-app-gallery/src/lib.rs`). This fixture
     * mirrors that list exactly so the test stays honest about what
     * production actually requests.
     */
    private val rustConnectRequestJson = """
        {
            "correlation_id": "0123456789abcdef00000001",
            "method": "get_public_key",
            "payload": "",
            "current_user": null,
            "counterparty": null,
            "permissions": [
                {"kind": "sign_event:0"},
                {"kind": "sign_event:1"},
                {"kind": "sign_event:3"},
                {"kind": "sign_event:5"},
                {"kind": "sign_event:6"},
                {"kind": "sign_event:7"},
                {"kind": "sign_event:13"},
                {"kind": "sign_event:16"},
                {"kind": "sign_event:1111"},
                {"kind": "sign_event:9802"},
                {"kind": "sign_event:10002"},
                {"kind": "sign_event:10003"},
                {"kind": "sign_event:10006"},
                {"kind": "sign_event:10050"},
                {"kind": "sign_event:30003"},
                {"kind": "sign_event:30004"},
                {"kind": "sign_event:39701"},
                {"kind": "nip44_encrypt"},
                {"kind": "nip44_decrypt"}
            ],
            "signer_package": "com.greenart7c3.nostrsigner",
            "force_interactive": false
        }
    """.trimIndent()

    @Test
    fun connectRequestDecodesAndSelectsIntentPath() {
        val request = json.decodeFromString<ExternalSignerRequest>(rustConnectRequestJson)
        assertEquals("get_public_key", request.method)
        assertEquals(19, request.permissions.size)
        // get_public_key is NOT a grantable batch permission — the first
        // connect always rides the Intent round-trip (user approval).
        assertFalse(shouldUseContentResolver(request))
    }

    @Test
    fun okReplyEncodesTheShapeRustParses() {
        // The reply the bridge builds from Amber's RESULT_OK — must decode
        // on the Rust side as ExternalSignerResponse{outcome: Ok{result},
        // signer_package: Some(..)} (serde tag = "kind", snake_case).
        val request = json.decodeFromString<ExternalSignerRequest>(rustConnectRequestJson)
        val response = ExternalSignerResponse(
            correlationId = request.correlationId,
            outcome = ExternalSignerOutcome.Ok(
                result = "npub1exampleexampleexampleexampleexampleexampleexampleexample",
            ),
            signerPackage = "com.greenart7c3.nostrsigner",
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), response)

        assertTrue(encoded.contains("\"correlation_id\":\"${request.correlationId}\""))
        assertTrue(encoded.contains("\"kind\":\"ok\""))
        assertTrue(encoded.contains("\"signer_package\":\"com.greenart7c3.nostrsigner\""))
        // Round-trips through the same wire shape unchanged (D7: verbatim).
        val decoded = json.decodeFromString<ExternalSignerResponse>(encoded)
        assertEquals(request.correlationId, decoded.correlationId)
        assertTrue(decoded.outcome is ExternalSignerOutcome.Ok)
    }

    @Test
    fun rejectedReplyEncodesTheShapeRustParses() {
        val response = ExternalSignerResponse(
            correlationId = "0123456789abcdef00000001",
            outcome = ExternalSignerOutcome.Rejected(reason = "user cancelled"),
        )
        val encoded = json.encodeToString(ExternalSignerResponse.serializer(), response)
        assertTrue(encoded.contains("\"kind\":\"rejected\""))
        assertTrue(encoded.contains("\"reason\":\"user cancelled\""))
    }

    @Test
    fun unavailableReplyTriggersRustForceInteractiveReissue() {
        // ContentResolver null → Unavailable; Rust re-issues the SAME op
        // with force_interactive = true, which must select the Intent path.
        val reissued = json.decodeFromString<ExternalSignerRequest>(
            """
            {
                "correlation_id": "0123456789abcdef00000002",
                "method": "sign_event",
                "payload": "{\"kind\":1,\"content\":\"hi\"}",
                "current_user": "deadbeef",
                "permissions": [],
                "granted_permissions": [{"kind": "sign_event:1"}],
                "signer_package": "com.greenart7c3.nostrsigner",
                "force_interactive": true
            }
            """.trimIndent(),
        )
        assertFalse(
            "force_interactive must override the granted-permission fast-path",
            shouldUseContentResolver(reissued),
        )
        // Same request WITHOUT the force flag rides the fast-path.
        val firstAttempt = reissued.copy(forceInteractive = false)
        assertTrue(shouldUseContentResolver(firstAttempt))
    }
}
