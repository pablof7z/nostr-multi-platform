// Test and test-support module declarations are kept out of `kernel/mod.rs`
// so the production kernel root remains below the hand-authored file cap.

#[cfg(test)]
mod action_failure_tests;
#[cfg(test)]
mod action_lifecycle_kernel_tests;
#[cfg(test)]
mod action_stages_tests;
#[cfg(test)]
mod action_terminal_correctness_tests;
#[cfg(test)]
mod cache_serve_all_kinds_dispatcher_tests;
#[cfg(test)]
mod cache_serve_budget_tests;
#[cfg(test)]
mod cache_serve_coverage_tests;
#[cfg(test)]
mod cache_serve_tag_hydration_tests;
#[cfg(test)]
mod cache_serve_tests;
#[cfg(test)]
mod cache_serve_universal_tests;
#[cfg(test)]
mod cache_serve_wakeup_tests;
#[cfg(test)]
mod cancel_correlation_tests;
#[cfg(test)]
mod chokepoint_tests;
#[cfg(test)]
mod claim_expansion_edge_tests;
#[cfg(test)]
mod claim_expansion_ingest_tests;
#[cfg(test)]
mod claim_expansion_seam;
#[cfg(test)]
mod claim_expansion_tests;
#[cfg(test)]
mod claim_expansion_tick_tests;
#[cfg(test)]
mod claimed_events_raw_author_tests;
#[cfg(test)]
mod clock_injection_tests;
#[cfg(test)]
mod closed_classifier_tests;
#[cfg(test)]
mod coverage_ledger_d1_tests;
#[cfg(all(test, feature = "native"))]
mod coverage_ledger_d2_journey_tests;
#[cfg(test)]
mod coverage_ledger_d2_tests;
#[cfg(test)]
mod d1_offline_bootstrap_tests;
#[cfg(test)]
mod dependent_interests_tests;
#[cfg(test)]
mod pointer_target_cache_serve_tests;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod dm_inbox_routing_tests;
#[cfg(test)]
mod eose_ok_notice_ingest_tests;
#[cfg(test)]
mod event_claim_hint_tests;
#[cfg(test)]
mod event_claim_released_tests;
#[cfg(test)]
mod event_claim_tests;
#[cfg(test)]
mod event_observer_tests;
#[cfg(test)]
mod gc_step_tests;
#[cfg(test)]
mod ingest_pre_verified_dispatcher_tests;
#[cfg(test)]
mod ingest_tests;
#[cfg(test)]
mod ingest_timeline_dispatcher_tests;
#[cfg(test)]
mod interest_install_cache_serve_support;
#[cfg(test)]
mod interest_install_cache_serve_tests;
#[cfg(test)]
mod interest_install_profile_cache_serve_tests;
#[cfg(any(test, feature = "test-support"))]
mod negentropy_test_support;
#[cfg(test)]
mod observer_replay_store_tests;
#[cfg(test)]
mod observer_replay_tests;
#[cfg(test)]
mod outbox_tests;
#[cfg(test)]
mod perf_tests;
#[cfg(test)]
mod proactive_profile_fetch_tests;
#[cfg(test)]
mod profile_claim_discovery_tests;
#[cfg(test)]
mod profile_claim_test_support;
#[cfg(test)]
mod profile_claim_tests;
#[cfg(test)]
mod provenance_wire_tests;
#[cfg(test)]
mod publish_completion_forget_tests; // D8 — forget handle↔correlation on completion (S7/#1754)
#[cfg(test)]
mod publish_engine_tests;
#[cfg(test)]
mod publish_event_echo_tests;
#[cfg(test)]
mod publish_relay_identity_tests;
#[cfg(test)]
mod publish_relay_receipt_tests;
#[cfg(test)]
mod publish_terminal_status_tests;
#[cfg(test)]
mod pull_cursor_retention_tests;
#[cfg(test)]
mod pull_cursor_wake_tests;
#[cfg(test)]
mod pull_relay_pin_tests;
#[cfg(test)]
mod pull_tests;
#[cfg(test)]
mod ram_eviction_tests;
#[cfg(test)]
mod ram_eviction_view_pin_tests;
#[cfg(test)]
mod recipient_publish_relays_tests;
#[cfg(test)]
mod refs_tests;
#[cfg(test)]
mod relay_score_tests;
#[cfg(test)]
mod router_composed_diagnostic_tests;
#[cfg(test)]
mod replaceable_ttl_gate_tests;
#[cfg(test)]
mod replay_tests;
#[cfg(test)]
mod retention_tests;
#[cfg(test)]
mod signed_events_return_tests;
#[cfg(test)]
mod snapshot_registry_tests;
#[cfg(test)]
mod state_projection_tests;
#[cfg(test)]
mod state_projection_profile_tests;
#[cfg(test)]
mod t142_drain_lifecycle_tick_tests;
#[cfg(test)]
mod nip46_relay_persistence_tests;
#[cfg(test)]
mod t170_relay_scoped_keying_tests;
#[cfg(test)]
mod t171_planner_error_projection_tests;
#[cfg(any(test, feature = "test-support"))]
mod test_router;
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
