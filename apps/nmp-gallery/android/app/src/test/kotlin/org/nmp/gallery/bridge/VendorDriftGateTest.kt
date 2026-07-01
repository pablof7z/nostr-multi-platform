package org.nmp.gallery.bridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * ADR-0048 Stage 2 — vendoring drift gate.
 *
 * The gallery is the canonical source of the `login-block` component; the CLI
 * install registry carries the in-repo vendored copy. The contract is
 * **byte-identical except the `package` declaration line** — any other
 * divergence is silent logic drift across the vendoring boundary and fails
 * here. External apps that vendor this component own their drift gates in their
 * own repositories.
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

    private val canonicalDir =
        "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry"

    /**
     * Every file of the vendored login-block unit. The bridge was split into
     * three files (file-size gate, PR #1183): wire types, Amber codec, bridge
     * core. ALL of them are vendored together — adding a canonical file
     * without adding it here (and to every copy location) is itself drift.
     */
    private val vendoredFiles = listOf(
        "ExternalSignerCapabilityBridge.kt",
        "ExternalSignerWire.kt",
        // Generated from the Rust catalog (#1493 P9) by `nmp gen signer-catalog`,
        // split out of ExternalSignerWire.kt. Like its sibling it is vendored
        // byte-identical-except-package across all copies — the codegen produces
        // identical content with only the `package` line differing per target.
        "KnownSigners.generated.kt",
        "AmberIntentCodec.kt",
        "NostrLoginBlock.kt",
    )

    /**
     * The web showcase no longer carries a hand-copied vendor fork: it reads the
     * Compose source straight out of the gated `registry.json` export (generated
     * from `crates/nmp-component-registry/registry/`, verified by `crates/nmp-cli/tests/export.rs`).
     * So the CLI registry copy below is the only remaining vendored copy on the
     * web path, and the gallery -> CLI -> registry.json chain stays fully gated.
     */
    private fun copyPaths(file: String): List<String> =
        listOf("crates/nmp-component-registry/registry/compose/login-block/$file")

    @Test
    fun allVendoredCopiesMatchGalleryCanonical() {
        for (file in vendoredFiles) {
            for (copy in copyPaths(file)) {
                assertIdenticalExceptPackage("$canonicalDir/$file", copy)
            }
        }
    }
}
