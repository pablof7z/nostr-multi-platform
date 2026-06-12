package org.nmp.android

// Vendored from apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/
// ExternalSignerCapabilityBridge.kt (canonical). Keep in sync with gallery version.
//
// The gallery is the single source of truth for the login-block component.
// Chirp vendors it here per the registry model: source is identical except for
// the package declaration.
//
// ADR-0048 Stage 2 — NIP-55 Kotlin host adapter.
//
// D7 contract: fires what Rust built; reports raw results; decides nothing.

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

// ── Wire types mirroring nmp-signer-iface ExternalSignerRequest/Response ──────

@Serializable
data class ExternalSignerRequest(
    @SerialName("correlation_id") val correlationId: String,
    val method: String,
    val payload: String,
    @SerialName("current_user") val currentUser: String? = null,
    val counterparty: String? = null,
    val permissions: List<Nip55Permission> = emptyList(),
    @SerialName("signer_package") val signerPackage: String? = null,
    @SerialName("force_interactive") val forceInteractive: Boolean = false,
)

@Serializable
data class Nip55Permission(val kind: String)

@Serializable
data class ExternalSignerResponse(
    @SerialName("correlation_id") val correlationId: String,
    val outcome: ExternalSignerOutcome,
    @SerialName("signer_package") val signerPackage: String? = null,
)

@Serializable
sealed class ExternalSignerOutcome {
    @Serializable
    @SerialName("ok")
    data class Ok(val result: String) : ExternalSignerOutcome()

    @Serializable
    @SerialName("rejected")
    data class Rejected(val reason: String) : ExternalSignerOutcome()

    @Serializable
    @SerialName("unavailable")
    data class Unavailable(val reason: String) : ExternalSignerOutcome()

    @Serializable
    @SerialName("signer_error")
    data class SignerError(val reason: String) : ExternalSignerOutcome()
}

// ── Known signer descriptors ──────────────────────────────────────────────────

data class NostrSignerInfo(
    val displayName: String,
    val intentScheme: String,
    val contentAuthority: String? = null,
    val installHint: String = "Install $displayName for one-tap sign-in",
)

val KNOWN_NOSTR_SIGNERS: List<NostrSignerInfo> = listOf(
    NostrSignerInfo(
        displayName = "Amber",
        intentScheme = "nostrsigner",
        contentAuthority = "com.greenart7c3.nostrsigner",
        installHint = "Install Amber for one-tap sign-in",
    ),
)

fun detectInstalledSigners(packageManager: PackageManager): List<NostrSignerInfo> {
    return KNOWN_NOSTR_SIGNERS.filter { signer ->
        val probe = Intent(Intent.ACTION_VIEW, Uri.parse("${signer.intentScheme}://"))
        @Suppress("DEPRECATION")
        val handlers = packageManager.queryIntentActivities(probe, PackageManager.MATCH_DEFAULT_ONLY)
        handlers.isNotEmpty()
    }
}

// ── Capability bridge ─────────────────────────────────────────────────────────

private val bridgeJson = Json {
    ignoreUnknownKeys = true
    isLenient = true
    classDiscriminator = "kind"
}

/**
 * D7 host adapter for the `external_signer` capability namespace.
 *
 * Receives `ExternalSignerRequest` objects from Rust, fires the right OS IPC
 * mechanism (Intent round-trip or ContentResolver fast-path), and reports raw
 * results back via `onResult`. It decides nothing (D7).
 *
 * ## Registration
 *
 * Call [register] in `Activity.onCreate` (before first `onStart`).
 * Call [unregister] in `Activity.onDestroy`.
 *
 * @param activity Host activity.
 * @param onResult Receives the serialised `ExternalSignerResponse` JSON.
 *   Route back to Rust via `KernelBridge.nativeDeliverSignerResponse`.
 */
class ExternalSignerCapabilityBridge(
    private val activity: ComponentActivity,
    private val onResult: (responseJson: String) -> Unit,
) {
    @Volatile private var pendingCorrelationId: String? = null
    @Volatile private var pendingMethod: String? = null

    private var launcher: ActivityResultLauncher<Intent>? = null

    fun register() {
        if (launcher != null) return
        launcher = activity.registerForActivityResult(
            ActivityResultContracts.StartActivityForResult(),
        ) { result ->
            val correlationId = pendingCorrelationId ?: return@registerForActivityResult
            pendingCorrelationId = null
            val method = pendingMethod ?: "unknown"
            pendingMethod = null

            val response = if (result.resultCode == Activity.RESULT_OK) {
                val data = result.data
                val rawResult = data?.getStringExtra("result")
                val signerPackage = data?.getStringExtra("package")
                if (rawResult != null) {
                    ExternalSignerResponse(
                        correlationId = correlationId,
                        outcome = ExternalSignerOutcome.Ok(result = rawResult),
                        signerPackage = signerPackage.takeIf {
                            method == "get_public_key" && it != null
                        },
                    )
                } else {
                    ExternalSignerResponse(
                        correlationId = correlationId,
                        outcome = ExternalSignerOutcome.Unavailable(
                            reason = "signer returned no result",
                        ),
                    )
                }
            } else {
                ExternalSignerResponse(
                    correlationId = correlationId,
                    outcome = ExternalSignerOutcome.Rejected(reason = "user cancelled"),
                )
            }
            onResult(bridgeJson.encodeToString(response))
        }
    }

    fun unregister() {
        launcher?.unregister()
        launcher = null
    }

    fun handle(request: ExternalSignerRequest) {
        val useContentResolver = !request.forceInteractive
            && request.signerPackage != null
            && request.permissions.any { p -> p.kind.startsWith(request.method.toPermissionKind()) }

        if (useContentResolver) {
            dispatchContentResolver(request)
        } else {
            dispatchIntent(request)
        }
    }

    fun handleJson(requestJson: String) {
        val request = try {
            bridgeJson.decodeFromString<ExternalSignerRequest>(requestJson)
        } catch (_: Exception) {
            return
        }
        handle(request)
    }

    private fun dispatchIntent(request: ExternalSignerRequest) {
        val methodTag = request.method.toNostrSignerMethod()
        val uriBuilder = StringBuilder("nostrsigner:$methodTag")
        uriBuilder.append("?compressionType=none&returnType=signature")
        uriBuilder.append("&type=$methodTag")
        if (request.currentUser != null) {
            uriBuilder.append("&current_user=${Uri.encode(request.currentUser)}")
        }
        if (request.counterparty != null) {
            uriBuilder.append("&pubkey=${Uri.encode(request.counterparty)}")
        }
        if (request.permissions.isNotEmpty()) {
            val permsJson = bridgeJson.encodeToString(request.permissions)
            uriBuilder.append("&permissions=${Uri.encode(permsJson)}")
        }
        if (request.payload.isNotEmpty()) {
            uriBuilder.append("&payload=${Uri.encode(request.payload)}")
        }

        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(uriBuilder.toString()))
        request.signerPackage?.let { pkg -> intent.setPackage(pkg) }
        intent.putExtra("nmp_correlation_id", request.correlationId)

        pendingCorrelationId = request.correlationId
        pendingMethod = request.method

        val l = launcher
        if (l != null) {
            l.launch(intent)
        } else {
            pendingCorrelationId = null
            pendingMethod = null
            val resp = ExternalSignerResponse(
                correlationId = request.correlationId,
                outcome = ExternalSignerOutcome.Unavailable(
                    reason = "capability bridge not registered",
                ),
            )
            onResult(bridgeJson.encodeToString(resp))
        }
    }

    private fun dispatchContentResolver(request: ExternalSignerRequest) {
        val pkg = request.signerPackage ?: run {
            reportUnavailable(request.correlationId, "signer package unknown for ContentResolver path")
            return
        }
        val method = request.method.toNostrSignerMethod()
        val authority = "$pkg.$method"
        val uri = Uri.parse("content://$authority")
        val selectionArgs = arrayOf(
            request.payload,
            request.counterparty ?: "",
            request.currentUser ?: "",
        )
        try {
            val cursor = activity.contentResolver.query(uri, null, null, selectionArgs, null)
            cursor?.use { c ->
                if (c.moveToFirst()) {
                    val resultCol = c.getColumnIndex("result")
                    val rawResult = if (resultCol >= 0) c.getString(resultCol) else null
                    if (rawResult != null) {
                        val resp = ExternalSignerResponse(
                            correlationId = request.correlationId,
                            outcome = ExternalSignerOutcome.Ok(result = rawResult),
                        )
                        onResult(bridgeJson.encodeToString(resp))
                    } else {
                        reportUnavailable(request.correlationId, "ContentResolver returned null result")
                    }
                } else {
                    reportUnavailable(request.correlationId, "ContentResolver returned empty cursor")
                }
            } ?: reportUnavailable(request.correlationId, "ContentResolver returned null cursor")
        } catch (e: Exception) {
            reportUnavailable(request.correlationId, "ContentResolver error: ${e.message}")
        }
    }

    private fun reportUnavailable(correlationId: String, reason: String) {
        val resp = ExternalSignerResponse(
            correlationId = correlationId,
            outcome = ExternalSignerOutcome.Unavailable(reason = reason),
        )
        onResult(bridgeJson.encodeToString(resp))
    }

    companion object {
        fun detect(context: Context): List<NostrSignerInfo> =
            detectInstalledSigners(context.packageManager)
    }
}

private fun String.toNostrSignerMethod(): String = when (this) {
    "get_public_key" -> "get_public_key"
    "sign_event" -> "sign_event"
    "nip44_encrypt" -> "nip44_encrypt"
    "nip44_decrypt" -> "nip44_decrypt"
    "nip04_encrypt" -> "nip04_encrypt"
    "nip04_decrypt" -> "nip04_decrypt"
    else -> this
}

private fun String.toPermissionKind(): String = when (this) {
    "sign_event" -> "sign_event:"
    "nip44_encrypt" -> "nip44_encrypt"
    "nip44_decrypt" -> "nip44_decrypt"
    "nip04_encrypt" -> "nip04_encrypt"
    "nip04_decrypt" -> "nip04_decrypt"
    else -> this
}
