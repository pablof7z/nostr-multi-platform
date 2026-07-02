use super::*;

#[test]
fn render_is_deterministic_for_all_platforms() {
    for platform in [Platform::Swift, Platform::Kotlin, Platform::Ts] {
        assert_eq!(render_feed_helpers(platform), render_feed_helpers(platform));
    }
}

#[test]
fn platform_parse_roundtrips() {
    assert_eq!(Platform::parse("swift").unwrap(), Platform::Swift);
    assert_eq!(Platform::parse("kotlin").unwrap(), Platform::Kotlin);
    assert_eq!(Platform::parse("ts").unwrap(), Platform::Ts);
    assert!(Platform::parse("rust").is_err());
}

#[test]
fn helpers_use_canonical_json_bridge_shape() {
    for platform in [Platform::Swift, Platform::Kotlin, Platform::Ts] {
        let rendered = render_feed_helpers(platform);
        assert!(rendered.contains("openFeedJson") || rendered.contains("feed_open_json"));
        assert!(rendered.contains("ActiveUserFollows"));
        assert!(rendered.contains("RootIndexed"));
        assert!(rendered.contains("Flat"));
        assert!(rendered.contains("NewestByFeedPosition"));
        assert!(rendered.contains("FeedRows"));
        assert!(rendered.contains("primary_kinds"));
        assert!(rendered.contains("source_page_size"));
    }
}

#[test]
fn helpers_cover_group_list_and_relay_set_families() {
    // #2723: generated helper coverage must extend beyond active-user-follows
    // so group-scoped, list-scoped, and relay-set-scoped consumers (29er, hl)
    // do not hand-write FeedParams literals.
    for platform in [Platform::Swift, Platform::Kotlin, Platform::Ts] {
        let rendered = render_feed_helpers(platform);
        assert!(
            rendered.contains("ActiveUserHostedGroups"),
            "{platform:?} missing hosted-groups family"
        );
        assert!(
            rendered.contains("ListMembers"),
            "{platform:?} missing list-members family"
        );
        assert!(
            rendered.contains("RelaySet"),
            "{platform:?} missing relay-set family"
        );
        assert!(
            rendered.to_lowercase().contains("hostedgroupsfeed"),
            "{platform:?} missing hosted-groups helper entry points"
        );
        assert!(
            rendered.to_lowercase().contains("listmembersfeed"),
            "{platform:?} missing list-members helper entry points"
        );
        assert!(
            rendered.to_lowercase().contains("relaysetfeed"),
            "{platform:?} missing relay-set helper entry points"
        );
    }
}

#[test]
fn generate_then_check_is_up_to_date() {
    for platform in [Platform::Swift, Platform::Kotlin, Platform::Ts] {
        let tmp = std::env::temp_dir().join(format!(
            "nmp-feed-helpers-roundtrip-{}-{:?}.gen",
            std::process::id(),
            platform
        ));
        generate_feed_helpers(platform, &tmp).unwrap();
        let outcome = check_feed_helpers(platform, &tmp).unwrap();
        assert!(
            outcome.up_to_date,
            "fresh-generated feed helper should be up to date"
        );
        let _ = std::fs::remove_file(&tmp);
    }
}

#[test]
fn checked_fixtures_are_current() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feed_helpers/generated");
    for (platform, file) in [
        (Platform::Swift, "FeedHelpers.generated.swift"),
        (Platform::Kotlin, "FeedHelpers.kt"),
        (Platform::Ts, "feedHelpers.generated.ts"),
    ] {
        let outcome = check_feed_helpers(platform, &root.join(file)).unwrap();
        assert!(
            outcome.up_to_date,
            "{file} feed-helper fixture is stale: {:?}",
            outcome.first_diff_line
        );
    }
}
