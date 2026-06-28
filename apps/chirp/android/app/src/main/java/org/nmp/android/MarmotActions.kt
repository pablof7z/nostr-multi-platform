package org.nmp.android

import android.util.Log
import java.util.UUID

private const val TAG = "MarmotActions"

/**
 * Marmot (MLS-over-Nostr encrypted groups) write operations — Android peer of
 * iOS `MarmotStore` (Bridge/MarmotBridge.swift). Extracted from [KernelModel]
 * to keep both files under the repo's 500-LOC hard ceiling.
 *
 * M14-1c / #2169: All dispatch paths now use the typed byte doorway via
 * `GeneratedActionBuilders.marmotXxx(...)` → [dispatchBytes]. The JSON DTO
 * classes (`MarmotActionEnvelopes.kt`) and the JSON bridge helper
 * (`dispatchMarmotAction`) are DELETED. No hand-spelled `"nmp.marmot"` literal
 * remains in production Kotlin (asserted by `ci/check_native_action_boundary.py`).
 *
 * Thin shell: ZERO protocol logic. Every op is a single generated FlatBuffers
 * buffer. State arrives reactively via the `nmp.marmot.snapshot` /
 * `nmp.marmot.messages` push projections on [KernelModel.state] (D8 — no poll).
 *
 * Call sites: [KernelModel.marmot] exposes this instance; UI screens reference
 * `model.marmot.createGroup(…)` etc., mirroring the iOS `model.marmot` surface.
 */
class MarmotActions(
    private val dispatchBytes: (bytes: ByteArray) -> DispatchResult,
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
    // Write operations (one generated builder per op)
    // ─────────────────────────────────────────────────────────────────────────

    /**
     * Publish (or rotate) the local MLS key-package (kind:30443). Fire-and-forget:
     * the refreshed key-package state arrives via the next snapshot tick.
     */
    fun publishKeyPackage(): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotPublishKeyPackage(
            correlationId = UUID.randomUUID().toString(),
        )
        return dispatch(bytes)
    }

    /**
     * Create a new MLS group. [inviteeText] is the raw text the user typed;
     * Rust tokenises (whitespace / comma / semicolon / newline) and validates
     * each entry — no parsing in Kotlin. Fire-and-forget: the new group appears
     * on the next snapshot tick.
     */
    fun createGroup(name: String, description: String, inviteeText: String): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotCreateGroup(
            correlationId = UUID.randomUUID().toString(),
            name = name,
            description = description,
            inviteeText = inviteeText.takeIf { it.isNotBlank() },
        )
        return dispatch(bytes)
    }

    /**
     * Invite peers to an existing MLS group. [inviteeText] is the raw text the
     * user typed; Rust tokenises and validates — no parsing in Kotlin.
     */
    fun invite(groupIdHex: String, inviteeText: String): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotInvite(
            correlationId = UUID.randomUUID().toString(),
            groupIdHex = groupIdHex,
            inviteeText = inviteeText.takeIf { it.isNotBlank() },
        )
        return dispatch(bytes)
    }

    /** Send an application message in an existing MLS group. */
    fun sendGroupMessage(groupIdHex: String, text: String): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotSend(
            correlationId = UUID.randomUUID().toString(),
            groupIdHex = groupIdHex,
            text = text,
        )
        return dispatch(bytes)
    }

    /**
     * Self-remove from a group (MLS SelfRemove proposal + commit). Mirrors iOS
     * `model.marmot.leave(groupIDHex:)`.
     */
    fun leave(groupIdHex: String): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotLeave(
            correlationId = UUID.randomUUID().toString(),
            groupIdHex = groupIdHex,
        )
        return dispatch(bytes)
    }

    /**
     * Remove other members from the group (MLS Remove proposal + commit).
     * [members] accepts raw hex pubkeys — PublicKey::parse accepts both hex and
     * npub, so snapshot member hex strings pass verbatim.
     */
    fun removeMembers(groupIdHex: String, members: List<String>): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotRemove(
            correlationId = UUID.randomUUID().toString(),
            groupIdHex = groupIdHex,
            memberNpubs = members,
        )
        return dispatch(bytes)
    }

    /** Accept a pending MLS group invite (kind:444 Welcome). */
    fun acceptWelcome(welcomeIdHex: String): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotAcceptWelcome(
            correlationId = UUID.randomUUID().toString(),
            welcomeIdHex = welcomeIdHex,
        )
        return dispatch(bytes)
    }

    /** Decline a pending MLS group invite. */
    fun declineWelcome(welcomeIdHex: String): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotDeclineWelcome(
            correlationId = UUID.randomUUID().toString(),
            welcomeIdHex = welcomeIdHex,
        )
        return dispatch(bytes)
    }

    /**
     * Explicit pending-commit clear — exposed so the UI can unwedge a group
     * after a relay-publish failure.
     */
    fun clearPending(groupIdHex: String): DispatchResult {
        val bytes = GeneratedActionBuilders.marmotClearPending(
            correlationId = UUID.randomUUID().toString(),
            groupIdHex = groupIdHex,
        )
        return dispatch(bytes)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Internal helpers
    // ─────────────────────────────────────────────────────────────────────────

    private fun dispatch(bytes: ByteArray): DispatchResult {
        val result = dispatchBytes(bytes)
        Log.d(TAG, "dispatchMarmotBytes(${bytes.size}B) → $result")
        return result
    }
}
