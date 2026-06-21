//! ADR-0063 Lane H: `resolved_profiles` projection deleted.
//!
//! All tests in this file (`claimed_profiles_fills_resolved_profiles`,
//! `resolved_profiles_present_and_empty_on_fresh_kernel`,
//! `unclaimed_pubkey_absent_from_resolved_profiles`,
//! `multiple_claimed_profiles_all_appear_in_resolved_profiles`) were testing
//! the `resolved_profiles` JSON snapshot projection, which is now removed.
//!
//! Profile resolution is now served by the `refs.profile` KPRF NRRD row-delta
//! sidecar (ADR-0063). Integration tests for profile delivery via `refs.profile`
//! live in the refs integration suite.
