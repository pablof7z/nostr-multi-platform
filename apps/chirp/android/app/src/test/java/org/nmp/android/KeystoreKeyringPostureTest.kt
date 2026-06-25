package org.nmp.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Keyring threat-model posture gate (issue #1201).
 *
 * The Android Keystore APIs (`KeyGenParameterSpec`, `KeyStore.getInstance("AndroidKeyStore")`)
 * are unavailable in the plain JVM unit-test harness, so the actual key-generation
 * path cannot be exercised here. The owner-resolved posture for the nsec-encrypting
 * AES-256-GCM key is instead enforced as a *source-level contract* — exactly the
 * gate style used by [ComponentVendorDriftGateTest]: the key-generation spec must
 * declare the agreed hardening, and must NOT declare the regressing alternative.
 *
 * Resolved posture (iOS parity — see docs/wiki/marmot-keyring.md):
 *  - `setUnlockedDeviceRequired(true)` — key usable only while the device is
 *    unlocked, WITHOUT a per-use biometric prompt. Behavioral analog of iOS
 *    `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`; compatible with cold-start
 *    identity restore. (API 28+.)
 *  - `setIsStrongBoxBacked(true)` preferred, with a runtime fallback that retries
 *    `build()` without StrongBox on `StrongBoxUnavailableException` (devices that
 *    lack a StrongBox security chip). (API 28+.)
 *  - Must NOT use `setUserAuthenticationRequired(true)` — that would force a
 *    biometric/PIN prompt before the kernel can restore identity, regressing
 *    below iOS and breaking headless cold-start restore.
 */
class KeystoreKeyringPostureTest {

    private val repoRoot: File by lazy {
        var dir = File(System.getProperty("user.dir")!!).absoluteFile
        while (!File(dir, "Cargo.lock").isFile || !File(dir, "crates/nmp-ffi").isDirectory) {
            dir = dir.parentFile
                ?: error("repo root not found above ${System.getProperty("user.dir")}")
        }
        dir
    }

    /**
     * Source of [KeystoreKeyringCapability] with comment lines stripped, so the
     * posture assertions match *actual builder invocations* rather than prose in
     * the doc comments (which legitimately name the rejected alternative).
     */
    private val source: String by lazy {
        val f = File(
            repoRoot,
            "apps/chirp/android/app/src/main/java/org/nmp/android/KeystoreKeyringCapability.kt",
        )
        assertTrue("missing KeystoreKeyringCapability.kt", f.exists())
        f.readLines()
            .map { it.trim() }
            .filterNot { it.startsWith("//") || it.startsWith("*") || it.startsWith("/*") }
            .joinToString("\n")
    }

    @Test
    fun keyIsUnlockedDeviceRequired() {
        assertTrue(
            "AES key spec must call setUnlockedDeviceRequired(true) (iOS-parity, " +
                "device-unlock-gated without a per-use biometric prompt)",
            source.contains("setUnlockedDeviceRequired(true)"),
        )
    }

    @Test
    fun keyPrefersStrongBoxWithFallback() {
        assertTrue(
            "AES key spec must prefer StrongBox via setIsStrongBoxBacked(true)",
            source.contains("setIsStrongBoxBacked(true)"),
        )
        assertTrue(
            "StrongBox preference must degrade gracefully via " +
                "StrongBoxUnavailableException fallback (retry build() without StrongBox)",
            source.contains("StrongBoxUnavailableException"),
        )
    }

    @Test
    fun keyDoesNotForceUserAuthentication() {
        assertFalse(
            "AES key spec must NOT call setUserAuthenticationRequired(true) — that " +
                "forces a biometric/PIN prompt before cold-start identity restore, " +
                "regressing below iOS parity and breaking headless restore",
            source.contains("setUserAuthenticationRequired(true)"),
        )
    }
}
