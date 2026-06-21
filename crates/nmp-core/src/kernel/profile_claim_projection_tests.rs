//! ADR-0063 Lane H: `claimed_profiles` projection deleted.
//!
//! All tests in this file (`warm_reclaim_reemits_profile_next_tick_with_no_req`,
//! `claimed_profiles_present_iff_claim_held`,
//! `multi_consumer_release_does_not_drop_resident_profile`) were testing the
//! `claimed_profiles` JSON snapshot projection, which is now removed.
//!
//! The warm-reclaim / REQ-deduplication invariants are covered by the
//! `profile_claim_tests.rs` suite (which tests outbound REQ behaviour, not the
//! projection map). Profile state delivery is now tested through the
//! `refs.profile` KPRF row-delta sidecar integration suite.
