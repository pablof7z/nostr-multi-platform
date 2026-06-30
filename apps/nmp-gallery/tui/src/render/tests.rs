use super::*;

#[test]
fn mention_chip_uses_reference_fallback() {
    let data = GalleryData::render_test_data();
    let profiles = LiveProfileMap::new();
    let lines = plain_lines("content-mention-chip", &data, &profiles, 80).join(" ");
    assert!(lines.contains("@fa984b…018f52"), "{lines}");
    assert!(!lines.contains("npub1"), "{lines}");
}

#[test]
fn kind_registry_embed_uses_real_reference_fallback() {
    let data = GalleryData::render_test_data();
    let profiles = LiveProfileMap::new();
    let lines = plain_lines("content-kind-registry", &data, &profiles, 80).join(" ");
    assert!(lines.contains("quote 276d69"), "{lines}");
    assert!(lines.contains("276d69"), "{lines}");
    assert!(!lines.contains("Quoted event body"), "{lines}");
}

#[test]
fn content_view_projects_nested_mention_preview() {
    let data = GalleryData::render_test_data();
    let lines = NostrContentView::new(&data.content_quote_card.tree)
        .render_data(Some(&data.content_quote_card.render_data))
        .lines(100)
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(lines.contains("quote 276d69"), "{lines}");
    assert!(lines.contains("276d69"), "{lines}");
    assert!(!lines.contains("Quoted event body"), "{lines}");
}

#[test]
fn user_components_claim_the_profile_shape_they_render() {
    let (pubkey, name_claims) = render_claims_for("user-name");
    assert_eq!(
        name_claims,
        BTreeSet::from([ProfileClaim {
            pubkey: pubkey.clone(),
            consumer_id: "tui/user-name".to_string(),
            shape: ProfileClaimShape::Ref,
        }])
    );

    let (_, nip05_claims) = render_claims_for("user-nip05");
    assert_eq!(
        nip05_claims,
        BTreeSet::from([ProfileClaim {
            pubkey: pubkey.clone(),
            consumer_id: "tui/user-nip05".to_string(),
            shape: ProfileClaimShape::Card,
        }])
    );

    let (_, card_claims) = render_claims_for("user-card");
    assert_eq!(
        card_claims,
        BTreeSet::from([ProfileClaim {
            pubkey,
            consumer_id: "tui/user-card".to_string(),
            shape: ProfileClaimShape::Card,
        }])
    );
}

fn render_claims_for(id: &str) -> (String, BTreeSet<ProfileClaim>) {
    let data = GalleryData::render_test_data();
    let pubkey = data.primary_pubkey.clone();
    let profiles = LiveProfileMap::new();
    let envelopes = BTreeMap::new();
    let claims = RefCell::new(BTreeSet::new());
    let embed_ctx = EmbedFrameContext {
        envelopes: &envelopes,
        sink: None,
        profile_claims: Some(&claims),
        consumer_id: "test",
        profiles: &profiles,
    };
    let area = Rect::new(0, 0, 80, 12);
    let mut buf = Buffer::empty(area);

    render_body(id, area, &mut buf, &data, embed_ctx);

    (pubkey, claims.into_inner())
}

// Embed-envelope projection tests live in `embed_host::tests` now. They
// exercise snapshot -> ClaimedEventDto -> EmbedKindProjection dispatch, not a
// static field on `ContentExample`.
