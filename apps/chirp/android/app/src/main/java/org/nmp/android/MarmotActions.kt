package org.nmp.android

import android.util.Log
import kotlinx.serialization.encodeToString

private const val TAG = "MarmotActions"

/**
 * Marmot (MLS-over-Nostr encrypted groups) write operations — Android peer of
 * iOS `MarmotStore` (Bridge/MarmotBridge.swift). Extracted from [KernelModel]
 * to keep both files under the repo's 500-LOC hard ceiling.
 *
 * Constructor takes [dispatchMarmotAction] — the typed Marmot write seam that
 * [KernelModel] owns. Thin shell: ZERO protocol logic. Every op is a single
 * Marmot action envelope; Rust owns validation, tokenisation, and key-package
 * resolution. State arrives reactively via the `nmp.marmot.snapshot` /
 * `nmp.marmot.messages` push projections on
 * [KernelModel.state] (D8 — no poll, no local echo).
 *
 * Call sites: [KernelModel.marmot] exposes this instance; UI screens reference
 * `model.marmot.createGroup(…)` etc., mirroring the iOS `model.marmot` surface.
 */
class MarmotActions(
    private val dispatchMarmotAction: (actionJson: String) -> DispatchResult,
) {
    /** Account this instance last registered a Marmot identity for. */
    private var registeredAccount: String? = null

    // ─────────────────────────────────────────────────────────────────────────
    // Registration
    // ─────────────────────────────────────────────────────────────────────────

    /**
     * Register a Marmot MLS identity against the active local account,
     * idempotent per account. [dbDir] is the host app-support directory (e.g.
     * `context.filesDir.path`). No-op when there is no active account yet, or
     * when already registered for the current account. Returns true when the
     * Rust side confirmed registration.
     *
     * Called by [KernelModel.registerMarmotIfNeeded] — not directly by UI.
     */
    internal fun registerIfNeeded(activeAccount: String, dbDir: String, bridge: KernelBridge): Boolean {
        if (activeAccount.isEmpty() || activeAccount == registeredAccount) return false
        val ok = bridge.marmotRegisterActive(dbDir)
        if (ok) registeredAccount = activeAccount
        return ok
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Write operations (one dispatchAction each)
    // ─────────────────────────────────────────────────────────────────────────

    /**
     * Create a new MLS group. [inviteeText] is the raw text the user typed;
     * Rust tokenises (whitespace / comma / semicolon / newline) and validates
     * each entry — no parsing in Kotlin. Fire-and-forget: the new group appears
     * on the next snapshot tick.
     */
    fun createGroup(name: String, description: String, inviteeText: String): DispatchResult {
        val envelope = MarmotCreateGroupEnvelope(
            name = name,
            description = description,
            inviteeText = inviteeText.takeIf { it.isNotBlank() },
        )
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    /** Send an application message in an existing MLS group. */
    fun sendGroupMessage(groupIdHex: String, text: String): DispatchResult {
        val envelope = MarmotSendEnvelope(groupIdHex = groupIdHex, text = text)
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    /** Publish (or rotate) the local MLS key package. */
    fun publishKeyPackage(): DispatchResult {
        val envelope = MarmotPublishKeyPackageEnvelope()
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    /** Accept a pending MLS group invite (kind:444 Welcome). */
    fun acceptWelcome(welcomeIdHex: String): DispatchResult {
        val envelope = MarmotAcceptWelcomeEnvelope(welcomeIdHex = welcomeIdHex)
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    /** Decline a pending MLS group invite. */
    fun declineWelcome(welcomeIdHex: String): DispatchResult {
        val envelope = MarmotDeclineWelcomeEnvelope(welcomeIdHex = welcomeIdHex)
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    /**
     * Self-remove from a group (MLS SelfRemove proposal + commit). Mirrors iOS
     * `model.marmot.leave(groupIDHex:)`.
     */
    fun leave(groupIdHex: String): DispatchResult {
        val envelope = MarmotLeaveEnvelope(groupIdHex = groupIdHex)
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    /**
     * Invite peers to an existing MLS group. [inviteeText] is the raw text the
     * user typed; Rust tokenises and validates — no parsing in Kotlin. Mirrors
     * iOS `model.marmot.invite(groupIDHex:inviteeText:)`.
     */
    fun invite(groupIdHex: String, inviteeText: String): DispatchResult {
        val envelope = MarmotInviteEnvelope(
            groupIdHex = groupIdHex,
            inviteeText = inviteeText.takeIf { it.isNotBlank() },
        )
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    /**
     * Remove other members from the group (MLS Remove proposal + commit).
     * [members] accepts raw hex pubkeys — PublicKey::parse accepts both hex and
     * npub, so snapshot member hex strings pass verbatim. Mirrors iOS
     * `model.marmot.remove(groupIDHex:memberNpubs:)`.
     */
    fun removeMembers(groupIdHex: String, members: List<String>): DispatchResult {
        val envelope = MarmotRemoveEnvelope(groupIdHex = groupIdHex, memberNpubs = members)
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    /**
     * Explicit pending-commit clear — exposed so the UI can unwedge a group
     * after a relay-publish failure. Mirrors iOS `model.marmot.clearPending`.
     */
    fun clearPending(groupIdHex: String): DispatchResult {
        val envelope = MarmotClearPendingEnvelope(groupIdHex = groupIdHex)
        return dispatch(chirpActionJson.encodeToString(envelope))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Internal helpers
    // ─────────────────────────────────────────────────────────────────────────

    private fun dispatch(actionJson: String): DispatchResult {
        val result = dispatchMarmotAction(actionJson)
        Log.d(TAG, "dispatchMarmotAction($actionJson) → $result")
        return result
    }
}
