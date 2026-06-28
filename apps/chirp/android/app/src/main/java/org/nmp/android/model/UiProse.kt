package org.nmp.android.model

/**
 * Shell-owned prose mapping objects (issue #1682 / #1735 / #2285) — the
 * presentation half of the codex ruling: Rust owns error/lifecycle semantics
 * (stable machine codes) + raw diagnostics; this shell owns the localized prose.
 *
 * Split out of `Snapshot.kt` (AGENTS.md 500-LOC hard ceiling). Same
 * `org.nmp.android.model` package, so the computed properties in `Snapshot.kt`
 * (`KernelUpdate.localizedErrorToast`, `ActionLifecycleEntry.localizedReason`)
 * reach these objects with no import.
 *
 * Android has no localized string-resource layer yet, so these return inlined
 * English copy (kept in lockstep with the iOS `NSLocalizedString` defaults); the
 * surface is wire-ready for a future `R.string` migration.
 */

/**
 * Maps a kernel `action_lifecycle` `reason_code` (#1735) to user-facing failure
 * copy — the Android parallel of iOS `UiLifecycleReasonProse`. The kernel ships a
 * stable code only for its OWN curated copy; opaque upstream / diagnostic text
 * stays prose-only (`reason_code` absent), so the caller falls back to the
 * English `reason` the wire carries. Returns `null` for an unrecognized key.
 */
object UiLifecycleReasonProse {
    fun localized(code: String, subject: String?): String? = when (code) {
        "lifecycle_no_active_account" -> "Sign in to an account first."
        "lifecycle_publish_no_explicit_target" ->
            "This private note needs an explicit relay to publish to."
        else -> null
    }
}

/**
 * Maps a kernel `last_error_category` code to user-facing error toast copy —
 * the Android parallel of iOS `UiErrorProse` (issue #1682 / #2285). The kernel
 * ships a stable code only for curated errors; unknown codes fall back to the
 * English `lastErrorToast`. Returns `null` for unrecognized keys.
 */
object UiErrorProse {
    fun localized(code: String, subject: String? = null): String? = when (code) {
        // nmp-nip17 (DM send)
        "nip17_dm_send_failed" -> "Couldn't send the message."
        "nip17_dm_giftwrap_failed" -> "Couldn't send the message — delivery failed."
        // nmp-nip47 (NWC wallet)
        "nip47_invalid_uri" -> "That wallet connection link isn't valid."
        "nip47_invalid_client_secret" -> "That wallet connection link is malformed."
        "nip47_req_encode_failed", "nip47_encrypt_failed",
        "nip47_sign_failed", "nip47_event_encode_failed" ->
            "Couldn't reach your wallet. Please try again."
        "nip47_wallet_error", "nip47_wallet_auth_error" -> "Your wallet reported an error."
        "nip47_wallet_not_ready" -> "Your wallet is still connecting."
        "nip47_wallet_not_connected" -> "No wallet is connected."
        "nip47_payment_aborted_no_durable_record" ->
            "Payment cancelled to keep it safe — please try again."
        // nmp-core (kernel / actor)
        "core_keyring_write_failed" ->
            "Couldn't save your sign-in securely — it may not persist."
        "core_relay_processing_error" -> "A relay update hit a snag — continuing."
        "signer_bunker_invalid_uri" -> "That remote signer link isn't valid."
        "signer_broker_not_initialised" -> "Remote signing isn't available right now."
        "signer_nip55_driver_not_initialised" -> "External signing isn't available right now."
        // nmp-nip57 (Zap)
        "nip57_zap_no_lnurl" -> "This user has no lightning address."
        "nip57_zap_lnurl_resolve_failed", "nip57_zap_fetch_failed", "nip57_zap_failed" ->
            "Zap failed. Please try again."
        "nip57_zap_sign_failed" -> "Couldn't sign the zap request."
        "nip57_zap_no_wallet" -> "No wallet connected — add a NWC wallet first."
        // nmp-nip05 (NIP-05 lookup)
        "nip05_lookup_invalid" -> "That NIP-05 identifier isn't valid."
        "nip05_lookup_failed" -> "NIP-05 lookup failed."
        "nip05_lookup_native_unavailable" -> "NIP-05 lookup isn't available in this build."
        else -> null
    }
}
