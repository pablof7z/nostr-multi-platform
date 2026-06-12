package org.nmp.gallery.bridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * ADR-0048 Stage 2 — vendoring drift gate.
 *
 * The gallery is the canonical source of the `login-block` component; Chirp,
 * the web registry, and the CLI install registry carry vendored copies. The
 * contract is **byte-identical except the `package` declaration line** — any
 * other divergence is silent logic drift across the vendoring boundary and
 * fails here.
 *
 * Cheap insurance recommended by the Opus review on PR #1153: the copies had
 * already diverged at first landing (491 vs 306 LOC); this gate makes that
 * impossible to repeat.
 */
class VendorDriftGateTest {

    private val repoRoot: File by lazy {
        var dir = File(System.getProperty("user.dir")!!).absoluteFile
        while (!File(dir, "AGENTS.md").exists()) {
            dir = dir.parentFile
                ?: error("repo root (AGENTS.md) not found above ${System.getProperty("user.dir")}")
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
        assertTrue("$copyPath: first line must be a package declaration", copy.first().startsWith("package "))
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

    @Test
    fun chirpBridgeCopyMatchesGalleryCanonical() {
        assertIdenticalExceptPackage(
            "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/ExternalSignerCapabilityBridge.kt",
            "android/app/src/main/java/org/nmp/android/ExternalSignerCapabilityBridge.kt",
        )
    }

    @Test
    fun webVendorBridgeCopyMatchesGalleryCanonical() {
        assertIdenticalExceptPackage(
            "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/ExternalSignerCapabilityBridge.kt",
            "web/registry/src/vendor/compose/login-block/ExternalSignerCapabilityBridge.kt",
        )
    }

    @Test
    fun cliRegistryBridgeCopyMatchesGalleryCanonical() {
        assertIdenticalExceptPackage(
            "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/ExternalSignerCapabilityBridge.kt",
            "crates/nmp-cli/registry/compose/login-block/ExternalSignerCapabilityBridge.kt",
        )
    }

    @Test
    fun webVendorLoginBlockCopyMatchesGalleryCanonical() {
        assertIdenticalExceptPackage(
            "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/NostrLoginBlock.kt",
            "web/registry/src/vendor/compose/login-block/NostrLoginBlock.kt",
        )
    }

    @Test
    fun cliRegistryLoginBlockCopyMatchesGalleryCanonical() {
        assertIdenticalExceptPackage(
            "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/NostrLoginBlock.kt",
            "crates/nmp-cli/registry/compose/login-block/NostrLoginBlock.kt",
        )
    }
}
