package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Vendoring drift gate for the registry Compose profile-component family.
 *
 * The registry (`crates/nmp-cli/registry/compose/...`) is the canonical source
 * of `NostrProfileHost` / `ProfileWire` / `NostrAvatar` / `NostrProfileName` /
 * `NostrNip05Badge`. Chirp installs vendored copies under
 * `org.nmp.android.components`. The contract — exactly as for the login-block
 * vendoring unit (ADR-0048 Stage 2, VendorDriftGateTest) — is **byte-identical
 * except the `package` declaration line**. Any other divergence is silent logic
 * drift across the vendoring boundary and fails here.
 *
 * Cheap insurance: makes it impossible for the Chirp copy to fork from the
 * registry source without the gate going red.
 */
class ComponentVendorDriftGateTest {

    private val repoRoot: File by lazy {
        var dir = File(System.getProperty("user.dir")!!).absoluteFile
        while (!File(dir, "Cargo.lock").isFile || !File(dir, "crates/nmp-cli").isDirectory) {
            dir = dir.parentFile
                ?: error("repo root not found above ${System.getProperty("user.dir")}")
        }
        dir
    }

    private fun read(path: String): List<String> {
        val f = File(repoRoot, path)
        assertTrue("missing vendored file: $path", f.exists())
        return f.readLines()
    }

    /** All lines must match except line 1, which must be a `package` decl. */
    private fun assertIdenticalExceptPackage(canonicalPath: String, copyPath: String) {
        val canonical = read(canonicalPath)
        val copy = read(copyPath)
        assertTrue(
            "$copyPath: first line must be a package declaration",
            copy.first().startsWith("package "),
        )
        assertTrue(
            "$canonicalPath: first line must be a package declaration",
            canonical.first().startsWith("package "),
        )
        assertEquals(
            "vendored copy drifted from canonical (line count): $copyPath",
            canonical.size,
            copy.size,
        )
        for (i in 1 until canonical.size) {
            assertEquals(
                "vendored copy drifted from canonical at $copyPath:${i + 1}",
                canonical[i],
                copy[i],
            )
        }
    }

    /** (registry source, Chirp vendored copy) pairs for the whole family. */
    private val vendoredPairs: List<Pair<String, String>> = listOf(
        "crates/nmp-cli/registry/compose/user-avatar/ProfileWire.kt" to
            "apps/chirp/android/app/src/main/java/org/nmp/android/components/ProfileWire.kt",
        "crates/nmp-cli/registry/compose/user-avatar/NostrProfileHost.kt" to
            "apps/chirp/android/app/src/main/java/org/nmp/android/components/NostrProfileHost.kt",
        "crates/nmp-cli/registry/compose/user-avatar/NostrAvatar.kt" to
            "apps/chirp/android/app/src/main/java/org/nmp/android/components/NostrAvatar.kt",
        "crates/nmp-cli/registry/compose/user-name/NostrProfileName.kt" to
            "apps/chirp/android/app/src/main/java/org/nmp/android/components/NostrProfileName.kt",
        "crates/nmp-cli/registry/compose/user-nip05/NostrNip05Badge.kt" to
            "apps/chirp/android/app/src/main/java/org/nmp/android/components/NostrNip05Badge.kt",
    )

    @Test
    fun vendoredProfileComponents_areByteIdenticalExceptPackageLine() {
        for ((canonical, copy) in vendoredPairs) {
            assertIdenticalExceptPackage(canonical, copy)
        }
    }
}
