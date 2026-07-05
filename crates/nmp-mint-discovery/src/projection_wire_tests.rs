//! Round-trip tests for [`MintDiscoveryProjection`]'s typed FlatBuffers codec.
//! Mirrors `nmp-wallet::projection_wire_tests` / `nmp-wot::wire::typed_fb`:
//! encode a projection, decode it back, assert structural equality.

use super::{
    decode_mint_discovery_projection, encode_mint_discovery_projection, FILE_IDENTIFIER,
    PROJECTION_KEY, SCHEMA_ID, SCHEMA_VERSION,
};
use crate::audit::{MintAuditRating, MintAuditSummary};
use crate::discovery::{DiscoveredMint, MintDiscoveryProjection};

fn fully_populated() -> MintDiscoveryProjection {
    MintDiscoveryProjection {
        mints: vec![
            DiscoveredMint {
                url: "https://mint.example".to_string(),
                name: Some("Example Mint".to_string()),
                icon_url: Some("https://mint.example/icon.png".to_string()),
                nuts: vec![1, 2, 4, 7, 11, 12],
                units: vec!["sat".to_string()],
                supports_nutzap: true,
                trust_score: 5,
                recommendation_count: 2,
                via_fallback: false,
                // The nested audit table never carries `icon_url` on the wire
                // (see `projection_wire.rs::decode_audit`'s doc comment) —
                // set it `None` here so the round-trip is exact.
                audit: Some(MintAuditSummary {
                    rating: MintAuditRating::Healthy,
                    success_rate: 0.99,
                    sampled_swaps: 40,
                    last_ok_at: Some("2026-07-04T00:00:00".to_string()),
                    icon_url: None,
                }),
            },
            DiscoveredMint {
                url: "https://mint.two".to_string(),
                name: None,
                icon_url: None,
                nuts: vec![],
                units: vec![],
                supports_nutzap: false,
                trust_score: 0,
                recommendation_count: 0,
                via_fallback: true,
                audit: None,
            },
        ],
    }
}

#[test]
fn round_trips_fully_populated_projection() {
    let projection = fully_populated();
    let bytes = encode_mint_discovery_projection(&projection);
    let decoded = decode_mint_discovery_projection(&bytes).expect("decode must succeed");
    assert_eq!(decoded, projection);
}

#[test]
fn round_trips_empty_projection() {
    let projection = MintDiscoveryProjection::default();
    let bytes = encode_mint_discovery_projection(&projection);
    let decoded = decode_mint_discovery_projection(&bytes).expect("decode must succeed");
    assert_eq!(decoded, projection);
    assert!(decoded.mints.is_empty());
}

#[test]
fn mint_rows_round_trip_with_and_without_optional_fields() {
    let projection = fully_populated();
    let bytes = encode_mint_discovery_projection(&projection);
    let decoded = decode_mint_discovery_projection(&bytes).expect("decode must succeed");

    let with_everything = &decoded.mints[0];
    assert_eq!(with_everything.name.as_deref(), Some("Example Mint"));
    assert_eq!(
        with_everything.icon_url.as_deref(),
        Some("https://mint.example/icon.png")
    );
    assert_eq!(with_everything.nuts, vec![1, 2, 4, 7, 11, 12]);
    assert_eq!(with_everything.units, vec!["sat".to_string()]);
    assert!(with_everything.supports_nutzap);
    assert_eq!(with_everything.trust_score, 5);
    assert_eq!(with_everything.recommendation_count, 2);
    assert!(!with_everything.via_fallback);
    let audit = with_everything.audit.as_ref().expect("audit must round-trip");
    assert_eq!(audit.rating, MintAuditRating::Healthy);
    assert!((audit.success_rate - 0.99).abs() < f64::EPSILON);
    assert_eq!(audit.sampled_swaps, 40);
    assert_eq!(audit.last_ok_at.as_deref(), Some("2026-07-04T00:00:00"));

    let bare = &decoded.mints[1];
    assert!(bare.name.is_none());
    assert!(bare.icon_url.is_none());
    assert!(bare.nuts.is_empty());
    assert!(bare.units.is_empty());
    assert!(bare.via_fallback);
    assert!(bare.audit.is_none());
}

#[test]
fn every_audit_rating_variant_round_trips() {
    for rating in [
        MintAuditRating::Unknown,
        MintAuditRating::Healthy,
        MintAuditRating::Degraded,
        MintAuditRating::Unreliable,
    ] {
        let projection = MintDiscoveryProjection {
            mints: vec![DiscoveredMint {
                url: "https://mint.example".to_string(),
                audit: Some(MintAuditSummary {
                    rating,
                    ..MintAuditSummary::default()
                }),
                ..DiscoveredMint::default()
            }],
        };
        let bytes = encode_mint_discovery_projection(&projection);
        let decoded = decode_mint_discovery_projection(&bytes).expect("decode must succeed");
        assert_eq!(decoded.mints[0].audit.as_ref().unwrap().rating, rating);
    }
}

#[test]
fn encoded_buffer_carries_the_nmds_file_identifier() {
    let bytes = encode_mint_discovery_projection(&fully_populated());
    assert!(
        super::generated::nmp::mint_discovery::mint_discovery_projection_buffer_has_identifier(
            &bytes
        )
    );
    assert_eq!(FILE_IDENTIFIER, b"NMDS");
}

#[test]
fn decode_rejects_buffer_without_identifier() {
    assert!(decode_mint_discovery_projection(&[]).is_err());
    assert!(decode_mint_discovery_projection(b"not a flatbuffer at all").is_err());
}

#[test]
fn schema_constants_are_stable() {
    assert_eq!(SCHEMA_ID, "nmp.mint_discovery");
    assert_eq!(PROJECTION_KEY, "mint_discovery");
    assert_eq!(SCHEMA_VERSION, 1);
}
