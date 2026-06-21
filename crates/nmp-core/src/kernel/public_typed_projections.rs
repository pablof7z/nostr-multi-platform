//! Public decode surface for the typed-projection sidecar (re-exported at the
//! crate root as `nmp_core::typed_projections`). The per-key decoders + their
//! typed DTOs let out-of-tree Rust consumers read typed projections instead of
//! string-keying the generic JSON `payload`. See the `typed_projections` module
//! doc for the return-type / scope rationale.
//!
//! Split from `kernel/mod.rs` to keep it under the file-size gate.
// ADR-0063 Lane H: decode_claimed_profiles / decode_resolved_profiles /
// ClaimedProfilesModel / ResolvedProfilesModel / CLAIMED_PROFILES_* /
// RESOLVED_PROFILES_* deleted. Profile resolution is now served by refs.profile.
pub use super::typed_projections::{
    // --- PR-B: newly-promoted decoders (identity + views + outbox cluster) ---
    // accounts
    decode_accounts,
    // --- already-public decode/model surface (wave-C diagnostics + publish) ---
    decode_action_results,
    decode_action_stages,
    // active_account
    decode_active_account,
    // claimed_events (nmp-gallery typed-sidecar migration — PR-B final zeroing)
    decode_claimed_events,
    // configured_relays
    decode_configured_relays,
    // profile (encode: ADR-0063 Lane F — build a refs.profile KPRF row payload)
    encode_profile,
    // outbox_summary
    decode_outbox_summary,
    // profile
    decode_profile,
    // V-112 (ADR-0042): decode_author_view, AuthorViewModel, ProfileActionModel,
    // ProfileDispatchSpecModel, AUTHOR_VIEW_* deleted.
    // V-112 (ADR-0042): decode_thread_view, ThreadViewModel, TimelineItemModel,
    // THREAD_VIEW_* deleted.
    // publish_outbox
    decode_publish_outbox,
    decode_publish_queue,
    decode_relay_diagnostics,
    // relay_role_options
    decode_relay_role_options,
    // settings_hub
    decode_settings_hub,
    // signed_events (nmp-ffi sign_event_for_return typed migration — PR-B final zeroing)
    decode_signed_events,
    AccountSummaryRow,
    AccountsModel,
    ActionResultRow,
    ActionResultsModel,
    ActionStageEntryRow,
    ActionStagesModel,
    ActiveAccountModel,
    ClaimedEventRow,
    ClaimedEventsModel,
    ConfiguredRelayRow,
    ConfiguredRelaysModel,
    InterestRow,
    OutboxSummaryModel,
    ProfileCardModel,
    PublishOutboxItemRow,
    PublishOutboxModel,
    PublishOutboxRelayRow,
    PublishQueueEntryRow,
    PublishQueueModel,
    RelayAckOutcomeRow,
    RelayDiagnosticsModel,
    RelayRoleOptionRow,
    RelayRoleOptionsModel,
    RelayRow,
    SettingsHubModel,
    SignedEventRow,
    SignedEventsModel,
    WireSubRow,
    ACCOUNTS_FILE_IDENTIFIER,
    ACCOUNTS_SCHEMA_ID,
    ACCOUNTS_SCHEMA_VERSION,
    ACTION_RESULTS_FILE_IDENTIFIER,
    ACTION_RESULTS_SCHEMA_ID,
    ACTION_RESULTS_SCHEMA_VERSION,
    ACTION_STAGES_FILE_IDENTIFIER,
    ACTION_STAGES_SCHEMA_ID,
    ACTION_STAGES_SCHEMA_VERSION,
    ACTIVE_ACCOUNT_FILE_IDENTIFIER,
    ACTIVE_ACCOUNT_SCHEMA_ID,
    ACTIVE_ACCOUNT_SCHEMA_VERSION,
    CLAIMED_EVENTS_FILE_IDENTIFIER,
    CLAIMED_EVENTS_SCHEMA_ID,
    CLAIMED_EVENTS_SCHEMA_VERSION,
    CONFIGURED_RELAYS_FILE_IDENTIFIER,
    CONFIGURED_RELAYS_SCHEMA_ID,
    CONFIGURED_RELAYS_SCHEMA_VERSION,
    OUTBOX_SUMMARY_FILE_IDENTIFIER,
    OUTBOX_SUMMARY_SCHEMA_ID,
    OUTBOX_SUMMARY_SCHEMA_VERSION,
    PROFILE_FILE_IDENTIFIER,
    PROFILE_SCHEMA_ID,
    PROFILE_SCHEMA_VERSION,
    PUBLISH_OUTBOX_FILE_IDENTIFIER,
    PUBLISH_OUTBOX_SCHEMA_ID,
    PUBLISH_OUTBOX_SCHEMA_VERSION,
    PUBLISH_QUEUE_FILE_IDENTIFIER,
    PUBLISH_QUEUE_SCHEMA_ID,
    PUBLISH_QUEUE_SCHEMA_VERSION,
    RELAY_DIAGNOSTICS_FILE_IDENTIFIER,
    RELAY_DIAGNOSTICS_SCHEMA_ID,
    RELAY_DIAGNOSTICS_SCHEMA_VERSION,
    RELAY_ROLE_OPTIONS_FILE_IDENTIFIER,
    RELAY_ROLE_OPTIONS_SCHEMA_ID,
    RELAY_ROLE_OPTIONS_SCHEMA_VERSION,
    SETTINGS_HUB_FILE_IDENTIFIER,
    SETTINGS_HUB_SCHEMA_ID,
    SETTINGS_HUB_SCHEMA_VERSION,
    SIGNED_EVENTS_FILE_IDENTIFIER,
    SIGNED_EVENTS_SCHEMA_ID,
    SIGNED_EVENTS_SCHEMA_VERSION,
};
// Actor-owned Tier-1 signer projections (closure-path, native-only).
// Promoted from `#[cfg(test)]` so external shells (chirp-desktop, Android) can
// decode the "signer_state", "bunker_handshake", and "nip46_onboarding" typed
// sidecars from snapshot frames (mirrors the Android #1286 gap fix).
#[cfg(feature = "native")]
pub use crate::actor::typed_projections::{
    decode_bunker_handshake, decode_nip46_onboarding, decode_signer_state, BunkerHandshakeModel,
    Nip46OnboardingModel, SignerAppRow, SignerStateModel, BUNKER_HANDSHAKE_SCHEMA_ID,
    NIP46_ONBOARDING_SCHEMA_ID, SIGNER_STATE_SCHEMA_ID,
};
// action_lifecycle — test-support only. The Tier-2 built-in decoder is not
// part of the default public surface (it is only needed in test helpers that
// read the typed sidecar after the generic JSON lane was removed).
// Import directly from the codec module (bypassing the pub(crate) re-export
// layer in typed_projections/mod.rs) so the public re-export here is valid.
#[cfg(any(test, feature = "test-support"))]
pub use super::typed_projections::action_lifecycle_fb::{
    decode_action_lifecycle, ActionLifecycleModel, LifecycleEntryRow,
    ACTION_LIFECYCLE_FILE_IDENTIFIER, ACTION_LIFECYCLE_SCHEMA_ID, ACTION_LIFECYCLE_SCHEMA_VERSION,
};
