package org.nmp.android

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import android.util.Base64
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

// ── Capability envelope contract (mirrors nmp-core substrate/capability.rs) ──

/**
 * Wire-shape of an incoming capability request from the kernel.
 * Mirrors `CapabilityRequest` from `nmp-core::substrate::capability`.
 */
@Serializable
private data class KeyringCapabilityRequest(
    val namespace: String,
    @SerialName("correlation_id") val correlationId: String,
    @SerialName("payload_json") val payloadJson: String,
)

/**
 * Wire-shape of the result handed back to the kernel.
 * Mirrors `CapabilityEnvelope` from `nmp-core::substrate::capability`.
 */
@Serializable
private data class KeyringCapabilityEnvelope(
    val namespace: String,
    @SerialName("correlation_id") val correlationId: String,
    @SerialName("result_json") val resultJson: String,
)

// ── Keyring vocabulary (mirrors nmp-core substrate/keyring.rs) ──────────────

@Serializable
private data class KeyringResult(
    val status: String,
    val secret: String? = null,
    @SerialName("os_status") val osStatus: Int? = null,
)

private object KeyringStatus {
    const val OK = "ok"
    const val NOT_FOUND = "not_found"
    const val ERROR = "error"
}

/**
 * Android Keystore AES-256-GCM implementation of the keyring capability.
 *
 * Mirrors iOS `KeychainCapability` — decode → execute → encode. Every failure
 * path returns a populated `result_json` envelope; this class never throws
 * across `handle(requestJson)` (D6).
 *
 * Storage model:
 * - Symmetric AES-256-GCM key is generated lazily inside the AndroidKeyStore
 *   hardware-backed store (non-exportable).
 * - Ciphertext and IV are base64-encoded and stored in app-private
 *   SharedPreferences keyed by `account_id`. The preferences file is
 *   `keyring_store` (MODE_PRIVATE).
 *
 * Do NOT use Jetpack security-crypto / EncryptedSharedPreferences — that
 * library is deprecated in Android 15+ and wraps the same AndroidKeyStore
 * primitives we use directly here.
 *
 * Doctrine:
 *  D6 — errors are data (`status:"error"`), never exceptions.
 *  D7 — this handler reports and executes; it decides no policy (which account
 *       is active, when to delete, etc. are Rust identity-layer decisions).
 */
class KeystoreKeyringCapability(context: Context) {

    private val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    companion object {
        const val NAMESPACE = "nmp.keyring.capability"

        private const val PREFS_NAME = "keyring_store"
        private const val KEY_ALIAS = "nmp_keyring_aes_key"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val GCM_TAG_LENGTH_BITS = 128

        // Separator between IV and ciphertext in the stored base64 value.
        private const val IV_CIPHERTEXT_SEP = ":"

        private val json = Json { ignoreUnknownKeys = true }

        private fun errorResult(code: Int = -1) =
            KeyringResult(status = KeyringStatus.ERROR, osStatus = code)

        private fun okResult(secret: String? = null) =
            KeyringResult(status = KeyringStatus.OK, secret = secret)

        private val notFoundResult =
            KeyringResult(status = KeyringStatus.NOT_FOUND)
    }

    /**
     * Synchronous capability handler — called by the Rust JNI trampoline via
     * `nativeSetCapabilityHandler`. The method signature `fun handle(String): String`
     * is the interface the Rust side invokes reflectively.
     *
     * D6: malformed input or any exception yields an error envelope; never throws.
     */
    fun handle(requestJson: String): String {
        val request = try {
            json.decodeFromString<KeyringCapabilityRequest>(requestJson)
        } catch (_: Exception) {
            // Malformed outer envelope — best-effort error response.
            val envelope = KeyringCapabilityEnvelope(
                namespace = NAMESPACE,
                correlationId = "",
                resultJson = json.encodeToString(errorResult(-50)),
            )
            return json.encodeToString(envelope)
        }

        // D7: this handler serves exactly one namespace. A request addressed to a
        // different namespace is a routing defect on the caller's side; reject it
        // as data (D6) rather than silently executing the keyring op against
        // foreign-namespace state. Echo back the request's own namespace and
        // correlation_id so the kernel dispatcher can correlate the rejection.
        if (request.namespace != NAMESPACE) {
            val envelope = KeyringCapabilityEnvelope(
                namespace = request.namespace,
                correlationId = request.correlationId,
                resultJson = json.encodeToString(errorResult(-51)),
            )
            return json.encodeToString(envelope)
        }

        val result = runCatching { process(request.payloadJson) }
            .getOrElse { errorResult(-1) }

        val envelope = KeyringCapabilityEnvelope(
            namespace = NAMESPACE,
            correlationId = request.correlationId,
            resultJson = json.encodeToString(result),
        )
        return json.encodeToString(envelope)
    }

    // ── Op dispatch ──────────────────────────────────────────────────────────

    private fun process(payloadJson: String): KeyringResult {
        val obj: JsonElement = try {
            json.parseToJsonElement(payloadJson)
        } catch (_: Exception) {
            return errorResult(-50)
        }

        val map = obj.jsonObject
        val op = map["op"]?.jsonPrimitive?.content ?: return errorResult(-50)
        val accountId = map["account_id"]?.jsonPrimitive?.content ?: return errorResult(-50)

        return when (op) {
            "store" -> {
                val secret = map["secret"]?.jsonPrimitive?.content ?: return errorResult(-50)
                store(accountId, secret)
            }
            "retrieve" -> retrieve(accountId)
            "delete" -> delete(accountId)
            else -> errorResult(-50)
        }
    }

    // ── Keystore operations ──────────────────────────────────────────────────

    private fun store(accountId: String, secret: String): KeyringResult {
        return try {
            val key = getOrCreateKey()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key)
            val iv = cipher.iv
            val ciphertext = cipher.doFinal(secret.toByteArray(Charsets.UTF_8))

            val ivB64 = Base64.encodeToString(iv, Base64.NO_WRAP)
            val ctB64 = Base64.encodeToString(ciphertext, Base64.NO_WRAP)
            prefs.edit().putString(accountId, "$ivB64$IV_CIPHERTEXT_SEP$ctB64").apply()
            okResult()
        } catch (_: Exception) {
            errorResult(-1)
        }
    }

    private fun retrieve(accountId: String): KeyringResult {
        val stored = prefs.getString(accountId, null) ?: return notFoundResult
        return try {
            val sep = stored.indexOf(IV_CIPHERTEXT_SEP)
            if (sep < 0) return errorResult(-50)
            val iv = Base64.decode(stored.substring(0, sep), Base64.NO_WRAP)
            val ciphertext = Base64.decode(stored.substring(sep + 1), Base64.NO_WRAP)

            val key = getOrCreateKey()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_LENGTH_BITS, iv))
            val plaintext = cipher.doFinal(ciphertext)
            val result = String(plaintext, Charsets.UTF_8)
            // Narrow the window where plaintext bytes reside in memory.
            // Full zeroization is impossible once a JVM String exists (immutable, interned),
            // but wiping the source ByteArray reduces exposure time.
            plaintext.fill(0)
            okResult(secret = result)
        } catch (_: Exception) {
            errorResult(-1)
        }
    }

    private fun delete(accountId: String): KeyringResult {
        // Idempotent — deleting an absent key is a no-op, not an error (D6).
        return try {
            prefs.edit().remove(accountId).apply()
            okResult()
        } catch (_: Exception) {
            errorResult(-1)
        }
    }

    // ── AndroidKeyStore key lifecycle ────────────────────────────────────────

    /**
     * Lazily generate (or load) the AES-256-GCM key from the AndroidKeyStore.
     * The key is non-exportable and never leaves the hardware-backed store.
     *
     * `@Synchronized` (instance-level mutual exclusion) closes the
     * check-then-generate race: without it, two concurrent first-callers can
     * both observe `containsAlias == false` and both call `generateKey()`, with
     * the second generation overwriting the alias. Any ciphertext written under
     * the first key would then fail to decrypt under the second. Serializing the
     * whole load-or-generate body makes the lazy init effectively-once.
     *
     * Threat-model posture (issue #1201 — iOS parity, see
     * `docs/wiki/marmot-keyring.md`):
     *  - `setUnlockedDeviceRequired(true)` (API 28+): the key is usable only
     *    while the device is unlocked, WITHOUT forcing a per-use biometric/PIN
     *    prompt. This is the behavioral analog of iOS
     *    `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` and is fully compatible
     *    with cold-start identity restore (the kernel restores identity after the
     *    user has unlocked the device to launch the app, so the key is available
     *    with no interaction).
     *  - `setIsStrongBoxBacked(true)` (API 28+) when a StrongBox security chip is
     *    present, falling back to TEE-backed generation on
     *    `StrongBoxUnavailableException` for devices without one.
     *  - We deliberately do NOT call `setUserAuthenticationRequired(true)`: that
     *    would force a biometric/PIN prompt before identity restore could run,
     *    regressing below iOS parity and breaking headless cold-start restore.
     */
    @Synchronized
    private fun getOrCreateKey(): SecretKey {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        if (keyStore.containsAlias(KEY_ALIAS)) {
            return (keyStore.getEntry(KEY_ALIAS, null) as KeyStore.SecretKeyEntry).secretKey
        }

        // Try StrongBox-backed generation first; degrade gracefully to the
        // standard TEE-backed key on devices without a StrongBox chip.
        return try {
            generateKey(strongBox = true)
        } catch (_: StrongBoxUnavailableException) {
            generateKey(strongBox = false)
        }
    }

    /**
     * Build the hardened [KeyGenParameterSpec] and generate the AES-256-GCM key.
     *
     * @param strongBox request StrongBox hardware backing (API 28+). When the
     *   device has no StrongBox, `build()`/`generateKey()` throws
     *   [StrongBoxUnavailableException], which the caller catches to retry with
     *   `strongBox = false`.
     */
    private fun generateKey(strongBox: Boolean): SecretKey {
        val keyGenerator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            ANDROID_KEYSTORE,
        )
        val builder = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setKeySize(256)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            // No biometric gate — the key must be accessible on cold start
            // for automatic identity restore without user interaction.
            .setUserAuthenticationRequired(false)

        // `setUnlockedDeviceRequired` / `setIsStrongBoxBacked` are API 28+; minSdk
        // is 26, so guard them. On API 26–27 the key is TEE-backed without the
        // device-unlocked constraint (best available on those platforms).
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            // iOS-parity: usable only while the device is unlocked, no per-use prompt.
            builder.setUnlockedDeviceRequired(true)
            if (strongBox) {
                builder.setIsStrongBoxBacked(true)
            }
        }

        keyGenerator.init(builder.build())
        return keyGenerator.generateKey()
    }
}
