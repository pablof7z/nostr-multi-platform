package org.nmp.registry

import android.app.Activity
import android.content.ContentResolver
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

/**
 * Mirror of `ExternalSignerRequest` from `nmp-signer-iface`.
 *
 * Rust builds this and serialises it as `CapabilityRequest.payload_json`.
 * The Kotlin host fires it and reports the raw result — it decides nothing (D7).
 */
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

/** Mirror of `Nip55Permission` from `nmp-signer-iface`. */
@Serializable
data class Nip55Permission(val kind: String)

/**
 * Mirror of `ExternalSignerResponse` from `nmp-signer-iface`.
 *
 * The host fills this and hands it back to Rust via `deliverResponse`.
 * D7: raw results only, no interpretation.
 */
@Serializable
data class ExternalSignerResponse(
    @SerialName("correlation_id") val correlationId: String,
    val outcome: ExternalSignerOutcome,
    @SerialName("signer_package") val signerPackage: String? = null,
)

/** Wire shape for `ExternalSignerOutcome` (tagged by `kind`). */
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

/**
 * Describes one locally-detectable Nostr signer app.
 *
 * Android detection: `PackageManager.queryIntentActivities` on the
 * `nostrsigner:` scheme. This is the Android analogue of the SwiftUI
 * `NostrSignerDetector.knownSigners` list.
 *
 * All package names listed here MUST also appear in the app's
 * `<queries>` block in `AndroidManifest.xml` — see the comment at the
 * top of the manifest. Without the `<queries>` declaration Android 11+
 * (API 30+) returns an empty result even when the app is installed.
 */
data class NostrSignerInfo(
    /** Display name shown in the login-block card (e.g. "Amber"). */
    val displayName: String,
    /**
     * The `nostrsigner:` scheme used for Intent dispatch.
     * Amber registers `nostrsigner`; future signers may differ.
     */
    val intentScheme: String,
    /**
     * ContentProvider authority prefix for the fast-path (background)
     * queries after the permission batch is granted. For Amber:
     * `com.greenart7c3.nostrsigner`. The full authority per-method is
     * `"$contentAuthority.<METHOD>"`, e.g.
     * `"com.greenart7c3.nostrsigner.sign_event"`.
     *
     * `null` means this signer supports the Intent path only.
     */
    val contentAuthority: String? = null,
    /**
     * Human-readable "not installed" hint for the UI.
     */
    val installHint: String = "Install $displayName for one-tap sign-in",
)

/**
 * Ordered list of signers this detector knows about.
 *
 * Extend this list as new Android Nostr signer apps emerge. Each entry
 * whose `intentScheme` is NOT resolvable by `PackageManager` is silently
 * excluded from the detection result; only installed apps surface.
 *
 * All `intentScheme` values here MUST be mirrored in `<queries>` in
 * `AndroidManifest.xml`.
 */
val KNOWN_NOSTR_SIGNERS: List<NostrSignerInfo> = listOf(
    NostrSignerInfo(
        displayName = "Amber",
        intentScheme = "nostrsigner",
        contentAuthority = "com.greenart7c3.nostrsigner",
        installHint = "Install Amber for one-tap sign-in",
    ),
)

// ── Package-manager-based detection ──────────────────────────────────────────

/**
 * Probes `PackageManager` for installed Nostr signer apps.
 *
 * Returns only signers whose `intentScheme` can be resolved by an
 * installed app. This mirrors the iOS `NostrSignerDetector.detect()`
 * approach (`UIApplication.canOpenURL`) but uses the Android
 * `PackageManager.queryIntentActivities` API instead.
 *
 * MUST be called on the main thread (same constraint as the iOS counterpart).
 *
 * ## AndroidManifest requirement
 *
 * Add the following `<queries>` block to your app's manifest:
 * ```xml
 * <queries>
 *     <intent>
 *         <action android:name="android.intent.action.VIEW" />
 *         <data android:scheme="nostrsigner" />
 *     </intent>
 * </queries>
 * ```
 * Without this declaration Android 11+ (API 30+) returns an empty list
 * even when Amber is installed.
 */
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
 * THE transport-selection rule (ADR-0048 D2) — a mechanical consequence of
 * fields Rust set on the request, never host policy (D7):
 *
 * | Condition | Mechanism |
 * |---|---|
 * | `forceInteractive == true` | Intent |
 * | `signerPackage` known AND method's permission in the request batch | ContentResolver |
 * | otherwise | Intent |
 *
 * Extracted as a pure top-level function so unit tests exercise the SAME
 * predicate `handle()` executes (no test-side copies).
 */
internal fun shouldUseContentResolver(request: ExternalSignerRequest): Boolean =
    !request.forceInteractive &&
        request.signerPackage != null &&
        request.permissions.any { p -> p.kind.startsWith(request.method.toPermissionKind()) }

/**
 * D7 host adapter for the `external_signer` capability namespace.
 *
 * Receives fully-built `ExternalSignerRequest` objects from Rust, fires
 * the right OS IPC mechanism (Intent round-trip or ContentResolver
 * fast-path), and reports raw results back via `onResult` — it decides
 * nothing.
 *
 * ## Transport selection (D7 — mechanical, not policy)
 *
 * | Condition | Mechanism |
 * |---|---|
 * | `forceInteractive == true` | Intent |
 * | method is in `permissions` (pre-granted) and `signerPackage` known | ContentResolver |
 * | otherwise | Intent |
 *
 * A ContentResolver returning `null` is reported as `Unavailable`; Rust
 * will re-issue the op with `force_interactive: true` to fall back to the
 * Intent path (D7 — the host never decides to retry).
 *
 * ## Lifecycle note
 *
 * The bridge registers an Activity Result launcher. Register it in
 * `Activity.onCreate` (before first `onStart`) via [register], not later.
 * Call [unregister] in `onDestroy` to release the launcher.
 *
 * @param activity The host activity (needed for `registerForActivityResult`).
 * @param onResult Called with the serialised `ExternalSignerResponse` JSON.
 *   Route this back to the kernel via `KernelBridge.deliverSignerResponse`.
 */
class ExternalSignerCapabilityBridge(
    private val activity: ComponentActivity,
    private val onResult: (responseJson: String) -> Unit,
) {

    // ── In-flight tracking ─────────────────────────────────────────────

    /** correlation_id of the request that is currently awaiting an Intent result. */
    @Volatile
    private var pendingCorrelationId: String? = null

    /** Method of the in-flight Intent (needed to build the response). */
    @Volatile
    private var pendingMethod: String? = null

    // ── Activity Result launcher ───────────────────────────────────────

    /**
     * Activity Result launcher registered for the `nostrsigner:` Intent.
     *
     * Android delivers the signer's reply as `Activity.RESULT_OK` with
     * extras:
     * - `"result"` — the raw string value (pubkey / signed-event JSON /
     *   ciphertext).
     * - `"package"` — the signer app's package name (present on
     *   `get_public_key` replies; Amber-specific).
     *
     * `RESULT_CANCELED` means the user navigated back without approving —
     * reported as `Rejected`.
     *
     * testTags are embedded in the Intent extras for the Stage-4 emulator
     * E2E (`adb shell am broadcast`) to inject fake results.
     */
    private var launcher: ActivityResultLauncher<Intent>? = null

    /**
     * Register the Activity Result launcher. Call from `Activity.onCreate`
     * BEFORE first `onStart`. Safe to call multiple times; subsequent calls
     * are no-ops.
     */
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
                    outcome = ExternalSignerOutcome.Rejected(
                        reason = "user cancelled",
                    ),
                )
            }
            onResult(bridgeJson.encodeToString(response))
        }
    }

    /**
     * Unregister the launcher. Call from `Activity.onDestroy`.
     * Safe to call when not registered.
     */
    fun unregister() {
        launcher?.unregister()
        launcher = null
    }

    // ── Dispatch ───────────────────────────────────────────────────────

    /**
     * Handle an `ExternalSignerRequest` built by Rust.
     *
     * Selects the transport path mechanically from `forceInteractive` +
     * `permissions`, then dispatches. D7: no policy decisions here.
     *
     * For the gallery showcase this is called with a stateless callback
     * wired to `onResult`. For Chirp it is wired into the kernel via
     * `nativeDeliverSignerResponse` (see `KernelBridge`).
     */
    fun handle(request: ExternalSignerRequest) {
        if (shouldUseContentResolver(request)) {
            dispatchContentResolver(request)
        } else {
            dispatchIntent(request)
        }
    }

    /**
     * Parse a raw `ExternalSignerRequest` JSON string and dispatch.
     * Called from the capability callback registered with the kernel.
     *
     * D6: malformed JSON is silently dropped (no crash); it degrades to
     * timeout on the Rust side (the correlation_id sender is never resolved).
     */
    fun handleJson(requestJson: String) {
        val request = try {
            bridgeJson.decodeFromString<ExternalSignerRequest>(requestJson)
        } catch (_: Exception) {
            return // D6: malformed — degrade to timeout
        }
        handle(request)
    }

    // ── Intent path ───────────────────────────────────────────────────

    private fun dispatchIntent(request: ExternalSignerRequest) {
        val methodTag = request.method.toNostrSignerMethod()
        // NIP-55 Intent URI: nostrsigner:<method>?<params>
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
        // Include the package hint so Amber auto-routes when multiple
        // nostrsigner-scheme handlers are installed.
        request.signerPackage?.let { pkg -> intent.setPackage(pkg) }
        // testTag for Stage-4 emulator E2E: the correlation_id is passed
        // as an extra so the adb-driven fake can echo it in RESULT_OK.
        intent.putExtra("nmp_correlation_id", request.correlationId)

        pendingCorrelationId = request.correlationId
        pendingMethod = request.method

        val l = launcher
        if (l != null) {
            l.launch(intent)
        } else {
            // Launcher not registered — report Unavailable so Rust can toast.
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

    // ── ContentResolver fast-path ─────────────────────────────────────

    private fun dispatchContentResolver(request: ExternalSignerRequest) {
        val pkg = request.signerPackage ?: run {
            reportUnavailable(request.correlationId, "signer package unknown for ContentResolver path")
            return
        }
        val method = request.method.toNostrSignerMethod()
        val authority = "$pkg.$method"
        val uri = Uri.parse("content://$authority")

        // NIP-55 ContentResolver call: the selection string carries the payload
        // and optional counterparty and current_user fields.
        val selectionArgs = arrayOf(
            request.payload,
            request.counterparty ?: "",
            request.currentUser ?: "",
        )

        try {
            val cursor = activity.contentResolver.query(
                uri,
                null,  // projection
                null,  // selection (Amber uses selectionArgs directly)
                selectionArgs,
                null,  // sortOrder
            )

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
                        // null result = silently-revoked permission. Report Unavailable;
                        // Rust re-issues with force_interactive = true.
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

    // ── Helpers ───────────────────────────────────────────────────────

    private fun reportUnavailable(correlationId: String, reason: String) {
        val resp = ExternalSignerResponse(
            correlationId = correlationId,
            outcome = ExternalSignerOutcome.Unavailable(reason = reason),
        )
        onResult(bridgeJson.encodeToString(resp))
    }

    companion object {
        /**
         * Detect installed Nostr signer apps using the PackageManager.
         * Convenience wrapper over [detectInstalledSigners].
         */
        fun detect(context: Context): List<NostrSignerInfo> =
            detectInstalledSigners(context.packageManager)
    }
}

// ── Method mapping helpers ────────────────────────────────────────────────────

/**
 * Map the Rust `ExternalSignerMethod` snake_case tag to the NIP-55
 * `nostrsigner:` method name (used in both the Intent URI and as the
 * ContentProvider suffix).
 *
 * Amber uses: `get_public_key`, `sign_event`, `nip44_encrypt`,
 * `nip44_decrypt`, `nip04_encrypt`, `nip04_decrypt`.
 */
private fun String.toNostrSignerMethod(): String = when (this) {
    "get_public_key" -> "get_public_key"
    "sign_event" -> "sign_event"
    "nip44_encrypt" -> "nip44_encrypt"
    "nip44_decrypt" -> "nip44_decrypt"
    "nip04_encrypt" -> "nip04_encrypt"
    "nip04_decrypt" -> "nip04_decrypt"
    else -> this
}

/**
 * Map a method tag to its corresponding NIP-55 permission kind string
 * used in the permission batch.
 *
 * E.g. `"sign_event"` → `"sign_event:"` (prefix for "sign_event:1" etc.),
 * `"nip44_encrypt"` → `"nip44_encrypt"`.
 */
private fun String.toPermissionKind(): String = when (this) {
    "sign_event" -> "sign_event:"
    "nip44_encrypt" -> "nip44_encrypt"
    "nip44_decrypt" -> "nip44_decrypt"
    "nip04_encrypt" -> "nip04_encrypt"
    "nip04_decrypt" -> "nip04_decrypt"
    else -> this
}
