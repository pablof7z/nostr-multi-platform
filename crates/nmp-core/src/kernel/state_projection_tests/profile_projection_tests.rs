//! `projections.profile` shape: no second metadata-source discriminator, and
//! raw hex pubkey only (no bech32 / npub encoding at the kernel layer).

use super::projection_fixtures_support::{snapshot, ACCOUNT};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

#[test]
fn profile_card_does_not_project_metadata_source() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    let snap = snapshot(&mut kernel);
    assert!(
        snap["projections"]["profile"]
            .get("metadata_source")
            .is_none(),
        "profile cards must not expose a second metadata-source discriminator"
    );
}

// `profile_card_projects_pending_kind0_publish_intent_after_restart` was
// deleted with the `local_profile_intents` overlay (#1193, ADR-0045 Rev 2
// single-mechanism). The overlay used to rehydrate an unsent pending kind:0
// from the publish store on kernel reconstruction; the retired architecture
// deliberately drops that publish-store-rehydration path. Read-your-writes for
// a locally-published kind:0 is now served immediately at publish time by
// `verify_and_persist` + `ingest_profile` into the canonical event store /
// `profiles` cache (covered by `local_kind0_publish_fans_out_to_event_observers`
// in `local_publish_intent_tests.rs`), not by a separate restart-restore overlay.

// V-112 (ADR-0042): d5_view_dependent_keys_absent_when_no_view_open deleted —
// author_view / thread_view projection bounding is removed with those projections.
// The open_author / open_thread methods and AuthorViewState / ThreadViewState are
// deleted from the kernel; per-app FlatFeed owns the view lifecycle.

// V-112 (ADR-0042): author_view_projects_edit_action_for_active_profile,
// author_view_projects_follow_action_for_non_active_profile,
// author_view_projects_unfollow_when_active_contacts_include_author,
// profile_action_follow_carries_nmp_follow_dispatch_spec,
// profile_action_unfollow_carries_nmp_unfollow_dispatch_spec,
// profile_action_edit_profile_has_no_dispatch_spec,
// author_view_carries_note_count_display_string — all deleted.
// author_view projection and profile_action_for() removed from kernel.

/// V-115 / ADR-0032: projection sends raw hex pubkey only; shells encode
/// bech32 and any abbreviation host-side. `npub` must be ABSENT from the
/// JSON projection.
#[test]
fn profile_card_carries_raw_pubkey_without_npub() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    let snap = snapshot(&mut kernel);
    let profile = &snap["projections"]["profile"];
    assert_eq!(
        profile["pubkey"].as_str(),
        Some(ACCOUNT),
        "profile.pubkey must carry the raw hex (aim.md §2)"
    );
    // ADR-0032 / V-115: `npub` bech32 field removed from projection.
    assert!(
        profile.get("npub").is_none(),
        "profile.npub must be absent — shells encode bech32 themselves"
    );
    assert!(
        profile.get("npub_short").is_none(),
        "npub_short field was removed by aim.md §2 — shells own abbreviation"
    );
}

// ADR-0063 Lane H: mention_profiles_projection_empty_when_no_visible_items_or_views
// deleted — mention_profiles projection removed entirely (replaced by refs.profile).
// ADR-0063 Lane H: claimed_profiles_projection_refines_claimed_pubkey deleted —
// claimed_profiles projection removed entirely (replaced by refs.profile KPRF
// row-delta sidecar). Profile refinement tests live in the refs integration suite.
