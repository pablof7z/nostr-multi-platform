//! Public decode surface for the typed-projection sidecar (re-exported at the
//! crate root as `nmp_core::typed_projections`). The per-key decoders + their
//! typed DTOs let out-of-tree Rust consumers read typed projections instead of
//! string-keying the generic JSON `payload`. See the `typed_projections` module
//! doc for the return-type / scope rationale.
//!
//! Split from `kernel/mod.rs` to keep it under the file-size gate.
pub use super::typed_projections::{
    // --- already-public decode/model surface (wave-C diagnostics + publish) ---
    decode_action_results, decode_action_stages, decode_publish_queue,
    decode_relay_diagnostics, ActionResultRow, ActionResultsModel, ActionStageEntryRow,
    ActionStagesModel, InterestRow, PublishQueueEntryRow, PublishQueueModel, RelayAckOutcomeRow,
    RelayDiagnosticsModel, RelayRow, WireSubRow, ACTION_RESULTS_FILE_IDENTIFIER,
    ACTION_RESULTS_SCHEMA_ID, ACTION_RESULTS_SCHEMA_VERSION, ACTION_STAGES_FILE_IDENTIFIER,
    ACTION_STAGES_SCHEMA_ID, ACTION_STAGES_SCHEMA_VERSION, PUBLISH_QUEUE_FILE_IDENTIFIER,
    PUBLISH_QUEUE_SCHEMA_ID, PUBLISH_QUEUE_SCHEMA_VERSION,
    RELAY_DIAGNOSTICS_FILE_IDENTIFIER, RELAY_DIAGNOSTICS_SCHEMA_ID,
    RELAY_DIAGNOSTICS_SCHEMA_VERSION,
    // --- PR-B: newly-promoted decoders (identity + views + outbox cluster) ---
    // accounts
    decode_accounts, AccountSummaryRow, AccountsModel, ACCOUNTS_FILE_IDENTIFIER,
    ACCOUNTS_SCHEMA_ID, ACCOUNTS_SCHEMA_VERSION,
    // active_account
    decode_active_account, ActiveAccountModel, ACTIVE_ACCOUNT_FILE_IDENTIFIER,
    ACTIVE_ACCOUNT_SCHEMA_ID, ACTIVE_ACCOUNT_SCHEMA_VERSION,
    // configured_relays
    decode_configured_relays, ConfiguredRelayRow, ConfiguredRelaysModel,
    CONFIGURED_RELAYS_FILE_IDENTIFIER, CONFIGURED_RELAYS_SCHEMA_ID,
    CONFIGURED_RELAYS_SCHEMA_VERSION,
    // settings_hub
    decode_settings_hub, SettingsHubModel, SETTINGS_HUB_FILE_IDENTIFIER,
    SETTINGS_HUB_SCHEMA_ID, SETTINGS_HUB_SCHEMA_VERSION,
    // profile
    decode_profile, ProfileCardModel, PROFILE_FILE_IDENTIFIER, PROFILE_SCHEMA_ID,
    PROFILE_SCHEMA_VERSION,
    // V-112 (ADR-0042): decode_author_view, AuthorViewModel, ProfileActionModel,
    // ProfileDispatchSpecModel, AUTHOR_VIEW_* deleted.
    // V-112 (ADR-0042): decode_thread_view, ThreadViewModel, TimelineItemModel,
    // THREAD_VIEW_* deleted.
    // publish_outbox
    decode_publish_outbox, PublishOutboxItemRow, PublishOutboxModel, PublishOutboxRelayRow,
    PUBLISH_OUTBOX_FILE_IDENTIFIER, PUBLISH_OUTBOX_SCHEMA_ID, PUBLISH_OUTBOX_SCHEMA_VERSION,
    // outbox_summary
    decode_outbox_summary, OutboxSummaryModel, OUTBOX_SUMMARY_FILE_IDENTIFIER,
    OUTBOX_SUMMARY_SCHEMA_ID, OUTBOX_SUMMARY_SCHEMA_VERSION,
    // resolved_profiles (desktop mention/display-name resolution; ProfileCardModel
    // re-used from the profile cluster above)
    decode_resolved_profiles, ResolvedProfilesModel, RESOLVED_PROFILES_FILE_IDENTIFIER,
    RESOLVED_PROFILES_SCHEMA_ID, RESOLVED_PROFILES_SCHEMA_VERSION,
    // claimed_profiles (V-112 follow-up: the direct observable of the
    // `claim_profile` verb — the app-template `validate_claim_profile`
    // example reads it; ProfileCardModel re-used from the profile cluster)
    decode_claimed_profiles, ClaimedProfilesModel, CLAIMED_PROFILES_FILE_IDENTIFIER,
    CLAIMED_PROFILES_SCHEMA_ID, CLAIMED_PROFILES_SCHEMA_VERSION,
    // claimed_events (nmp-gallery typed-sidecar migration — PR-B final zeroing)
    decode_claimed_events, ClaimedEventRow, ClaimedEventsModel,
    CLAIMED_EVENTS_FILE_IDENTIFIER, CLAIMED_EVENTS_SCHEMA_ID, CLAIMED_EVENTS_SCHEMA_VERSION,
    // signed_events (nmp-ffi sign_event_for_return typed migration — PR-B final zeroing)
    decode_signed_events, SignedEventRow, SignedEventsModel, SIGNED_EVENTS_FILE_IDENTIFIER,
    SIGNED_EVENTS_SCHEMA_ID, SIGNED_EVENTS_SCHEMA_VERSION,
};
// Actor-owned Tier-1 signer projections (closure-path, native-only).
// Promoted from `#[cfg(test)]` so external shells (chirp-desktop, Android) can
// decode the "signer_state", "bunker_handshake", and "nip46_onboarding" typed
// sidecars from snapshot frames (mirrors the Android #1286 gap fix).
#[cfg(feature = "native")]
pub use crate::actor::typed_projections::{
    decode_bunker_handshake, decode_nip46_onboarding, decode_signer_state,
    BunkerHandshakeModel, Nip46OnboardingModel, SignerAppRow, SignerStateModel,
    BUNKER_HANDSHAKE_SCHEMA_ID, NIP46_ONBOARDING_SCHEMA_ID, SIGNER_STATE_SCHEMA_ID,
};
