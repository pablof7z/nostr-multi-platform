//! Profile-targeted interest-install cache-serve regression tests.

use super::cache_serve_tests::{drain_cache_serves, simulate_cold_restart};
use super::interest_install_cache_serve_support::{seed_kind0_event, CapturingParser};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Opening a `nostr:` URI installs an interest for the resolved target. Before
/// the F2 fix, `open_uri` called bare `ensure_sub` with neither a recompile
/// trigger nor a store-cache serve.
#[test]
fn open_uri_serves_store_for_resolved_target() {
    use crate::app::{KernelAction, KernelUpdate};
    use crate::kernel_action::dispatch_kernel_action;
    use crate::nip19::encode_npub;

    let base_ts: u64 = 1_760_000_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let parser = CapturingParser::new();
    kernel.register_ingest_parser(0, parser.clone());
    let meta_id = seed_kind0_event(&mut kernel, &keys, base_ts);
    assert!(
        parser.seen_ids().contains(&meta_id),
        "Phase 1: parser must see the kind:0 event on live ingest"
    );

    simulate_cold_restart(&mut kernel);
    parser.clear();
    assert!(kernel.events.is_empty());

    let npub = encode_npub(&author).expect("valid npub");
    let update = dispatch_kernel_action(
        &mut kernel,
        KernelAction::OpenUri {
            uri: format!("nostr:{npub}"),
        },
    );
    assert!(
        matches!(update, KernelUpdate::ViewOpened { .. }),
        "open_uri must resolve the npub to a profile view; got {update:?}"
    );
    drain_cache_serves(&mut kernel, 10);

    assert!(
        parser.seen_ids().contains(&meta_id),
        "OPEN-URI BYPASS FAIL: parser must receive the store-resident kind:0 \
         event ({meta_id}) after open_uri installs the profile interest; \
         got {:?}",
        parser.seen_ids()
    );
}

/// `register_profile_claim_interest` routes through the unified front-door.
/// This proves a stored kind:0 event populates the ProfileCache on a cold-cache
/// kernel immediately after `resolve_ref`.
#[test]
fn profile_claim_serves_stored_kind0_from_store_on_cold_cache() {
    use crate::kernel::refs::{ProfileShape, RefLiveness, RefNamespace, RefShape};
    use crate::substrate::{ProfileLookup, TestKind0Parser, TestProfileCache};

    let base_ts: u64 = 1_770_000_000;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let meta_id = seed_kind0_event(&mut kernel, &keys, base_ts);

    simulate_cold_restart(&mut kernel);
    assert!(
        kernel.events.is_empty(),
        "events cache must be empty after restart"
    );

    let cold_cache = std::sync::Arc::new(TestProfileCache::new());
    kernel.set_profile_lookup(
        std::sync::Arc::clone(&cold_cache) as std::sync::Arc<dyn ProfileLookup>
    );
    kernel.register_ingest_parser(
        0,
        std::sync::Arc::new(TestKind0Parser::new(std::sync::Arc::clone(&cold_cache))),
    );

    assert!(
        cold_cache.profile(&author).is_none(),
        "Pre-condition: cold profile cache must not contain the author"
    );

    kernel.resolve_ref(
        RefNamespace::Profile,
        author.clone(),
        "test-consumer".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
        false,
        Vec::new(),
    );
    drain_cache_serves(&mut kernel, 10);

    assert!(
        cold_cache.profile(&author).is_some(),
        "PROFILE-CLAIM STORE-FIRST FAIL: profile_lookup().profile(P) must be Some \
         after resolve_ref installs the kind:0 interest and the cache-serve \
         runs from the store; got None. This is the cold-cache kind:0 bug \
         (timeline shows only pubkeys after relaunch). Stored event: {meta_id}."
    );
}
