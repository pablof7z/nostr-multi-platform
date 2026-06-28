package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Production cold-start identity-persistence regression guard.
 *
 * The Rust kernel restores the persisted identity automatically inside
 * `ActorCommand::Start` (`session_persistence::restore_active_session`) and
 * persists on sign-in (`enqueue_persist_current_active_session`). BOTH read and
 * write the secret exclusively through the keyring **capability** socket. If the
 * host never installs the `KeystoreKeyringCapability`, the persist writes go
 * nowhere and the restore on the next launch finds nothing — so a signed-in user
 * is silently logged out on every restart.
 *
 * This previously regressed because the keyring capability + identity-restore
 * were only wired on the `startWithContext` path, which production never reached
 * (it was gated behind `BuildConfig.DEBUG` test-extra injection). iOS installs
 * the keychain capability unconditionally in `KernelModel.init` and restores
 * unconditionally in `start()`; Android must mirror that.
 *
 * These tests drive the pure [planKernelLaunch] orchestration with a recording
 * [RecordingLaunchSeam] so they run on the JVM without the native `.so`.
 */
class KernelLaunchSequenceTest {

    /**
     * Records every launch-relevant operation in invocation order so the test
     * can assert the production path installs the keyring capability AND issues
     * identity-restore.
     */
    private class RecordingLaunchSeam : KernelLaunchSeam {
        val ops = mutableListOf<String>()
        var capabilityInstalled = false
        var restoreDbDir: String? = null
        var restoreTestNsec: String? = null
        var seededRelays: String? = null
        var startStoragePath: String? = null

        override fun installKeyringCapability() {
            capabilityInstalled = true
            ops += "installKeyringCapability"
        }

        override fun startKernel(storagePath: String?, testRelays: String?) {
            startStoragePath = storagePath
            seededRelays = testRelays
            ops += "startKernel"
        }

        override fun wireListeners() {
            ops += "wireListeners"
        }

        override fun identityRestore(dbDir: String, testNsec: String?) {
            restoreDbDir = dbDir
            restoreTestNsec = testNsec
            ops += "identityRestore"
        }
    }

    // ── Production path — the regression guard ────────────────────────────────

    @Test
    fun productionLaunchInstallsCapabilityAndRestoresIdentity() {
        val seam = RecordingLaunchSeam()
        planKernelLaunch(
            seam = seam,
            storagePath = "/data/NMP",
            dbDir = "/data",
            testNsec = null,
            testRelays = null,
        )

        assertTrue(
            "production launch MUST install the keyring capability " +
                "(else persist + restore are silent no-ops)",
            seam.capabilityInstalled,
        )
        assertTrue(
            "production launch MUST call identityRestore so a signed-in user " +
                "survives a cold restart",
            seam.ops.contains("identityRestore"),
        )
        // In production no test nsec is injected; restore reads the persisted
        // secret from the keyring capability instead.
        assertNull(seam.restoreTestNsec)
        assertEquals("/data", seam.restoreDbDir)
        // No test-relay override in production: the kernel seeds Chirp defaults.
        assertNull(seam.seededRelays)
    }

    @Test
    fun capabilityIsInstalledBeforeRestore() {
        val seam = RecordingLaunchSeam()
        planKernelLaunch(
            seam = seam,
            storagePath = null,
            dbDir = "/data",
            testNsec = null,
            testRelays = null,
        )
        val installIdx = seam.ops.indexOf("installKeyringCapability")
        val restoreIdx = seam.ops.indexOf("identityRestore")
        assertTrue("capability install must occur", installIdx >= 0)
        assertTrue("restore must occur", restoreIdx >= 0)
        assertTrue(
            "keyring capability must be installed BEFORE identity-restore " +
                "reads the persisted secret",
            installIdx < restoreIdx,
        )
        // Restore must also occur after the kernel is started (the actor's
        // Start command performs the synchronous restore-read chain).
        assertTrue(
            "kernel must be started before restore",
            seam.ops.indexOf("startKernel") < restoreIdx,
        )
    }

    // ── Test-injection path — layered on top, never a separate orchestration ──

    @Test
    fun testInjectionLayersOnTopOfTheSameProductionPath() {
        val seam = RecordingLaunchSeam()
        planKernelLaunch(
            seam = seam,
            storagePath = "/data/NMP",
            dbDir = "/data",
            testNsec = "nsec1testtesttest",
            testRelays = """[["ws://127.0.0.1:10547","both"]]""",
        )
        // Same unconditional capability + restore wiring as production…
        assertTrue(seam.capabilityInstalled)
        assertTrue(seam.ops.contains("identityRestore"))
        // …plus the injected test seams ride along.
        assertEquals("nsec1testtesttest", seam.restoreTestNsec)
        assertEquals("""[["ws://127.0.0.1:10547","both"]]""", seam.seededRelays)
    }
}
