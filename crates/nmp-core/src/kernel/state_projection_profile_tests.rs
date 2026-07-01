//! Profile-card projection coverage split from the broader snapshot tests.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const ACCOUNT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn snapshot(kernel: &mut Kernel) -> serde_json::Value {
    let json = kernel.make_update_json_for_test(true);
    serde_json::from_str(&json).expect("kernel snapshot must be valid JSON")
}

#[test]
fn profile_metadata_appears_in_snapshot_after_kind0_ingest() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ACCOUNT.to_string());

    let before = snapshot(&mut kernel);
    assert!(
        before["projections"]["profile"]["display_name"].is_null(),
        "before any kind:0 the profile card display_name must be null",
    );
    assert!(
        before["projections"]["profile"]["picture_url"].is_null(),
        "before any kind:0 the profile card picture_url must be null",
    );

    let raw_fields = serde_json::from_value(serde_json::json!({
        "name": "sat",
        "display_name": "Satoshi",
        "displayName": "Satoshi Camel",
        "nip05": "sat@example.com",
        "about": "hi there",
        "picture": "https://example.com/sat.png",
        "banner": "https://example.com/banner.png",
        "website": "https://satoshi.example",
        "lud16": "sat@ln.example",
        "lud06": "lnurl1sat",
    }))
    .expect("profile raw fields");
    kernel.seed_profile_view_for_test(
        ACCOUNT,
        crate::substrate::ProfileView {
            event_id: "0000000000000000000000000000000000000000000000000000000000000010"
                .to_string(),
            created_at: 1_700_000_000,
            display: "Satoshi".to_string(),
            name: Some("sat".to_string()),
            raw_display_name: Some("Satoshi".to_string()),
            display_name_camel: Some("Satoshi Camel".to_string()),
            nip05: "sat@example.com".to_string(),
            about: "hi there".to_string(),
            picture_url: Some("https://example.com/sat.png".to_string()),
            banner: Some("https://example.com/banner.png".to_string()),
            website: Some("https://satoshi.example".to_string()),
            lud16: Some("sat@ln.example".to_string()),
            lud06: Some("lnurl1sat".to_string()),
            lnurl: Some("sat@ln.example".to_string()),
            raw_fields,
        },
    );

    let after = snapshot(&mut kernel);
    let card = &after["projections"]["profile"];
    assert_eq!(card["display_name"].as_str(), Some("Satoshi"));
    assert_eq!(card["name"].as_str(), Some("sat"));
    assert_eq!(card["raw_display_name"].as_str(), Some("Satoshi"));
    assert_eq!(card["display_name_camel"].as_str(), Some("Satoshi Camel"));
    assert_eq!(
        card["picture_url"].as_str(),
        Some("https://example.com/sat.png"),
        "kind:0 picture must be projected into profile.picture_url",
    );
    assert_eq!(
        card["banner"].as_str(),
        Some("https://example.com/banner.png")
    );
    assert_eq!(card["website"].as_str(), Some("https://satoshi.example"));
    assert_eq!(card["nip05"].as_str(), Some("sat@example.com"));
    assert_eq!(card["lud16"].as_str(), Some("sat@ln.example"));
    assert_eq!(card["lud06"].as_str(), Some("lnurl1sat"));
    assert_eq!(after["metrics"]["profile_events"].as_u64(), Some(1));
}
