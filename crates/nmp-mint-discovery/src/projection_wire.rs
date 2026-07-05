//! Typed FlatBuffers wire codec for [`crate::discovery::MintDiscoveryProjection`]
//! (issue #2880, extracted into this crate).
//!
//! `nmp_mint_discovery::register` registers this under the projection key
//! `"mint_discovery"` (see `register.rs`), a fresh framework key distinct from
//! any host/wallet-owned sidecar — any app that composes this crate gets the
//! projection, whether or not it also composes a wallet.
//!
//! The schema (`crates/nmp-mint-discovery/schema/mint_discovery.fbs`) mirrors
//! the Rust structs field-for-field. `Option<...>` fields carry a `has_*`
//! presence flag plus the value so absent (`None`) round-trips distinctly
//! from a present default.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed
//! input; there are no `unwrap`/`expect`/panicking-index operations on the
//! decode path.

// The generated FlatBuffers bindings are intrinsically `unsafe` (every
// accessor reads from a raw `Table`). This `allow` block scopes the
// relaxation to the single generated module — no hand-written code in this
// file uses `unsafe`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/mint_discovery_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, ForwardsUOffset, Vector, WIPOffset};

use generated::nmp::mint_discovery as fb;

use crate::audit::{MintAuditRating, MintAuditSummary};
use crate::discovery::{DiscoveredMint, MintDiscoveryProjection};

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.mint_discovery";
/// The projection key this typed sidecar registers under.
pub const PROJECTION_KEY: &str = "mint_discovery";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NMDS";
/// Wire schema version. Bump on any breaking change to `mint_discovery.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- encode ---------------------------------------------------------------

/// Encode a [`MintDiscoveryProjection`] to typed FlatBuffers bytes (with the
/// `NMDS` file identifier).
#[must_use]
pub fn encode_mint_discovery_projection(projection: &MintDiscoveryProjection) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let mints = encode_mints(&mut fbb, &projection.mints);
    let root = fb::MintDiscoveryProjection::create(
        &mut fbb,
        &fb::MintDiscoveryProjectionArgs {
            schema_version: SCHEMA_VERSION,
            mints: Some(mints),
        },
    );
    fb::finish_mint_discovery_projection_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_mints<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    mints: &[DiscoveredMint],
) -> WIPOffset<Vector<'a, ForwardsUOffset<fb::DiscoveredMintRow<'a>>>> {
    let rows: Vec<_> = mints
        .iter()
        .map(|mint| {
            let url = fbb.create_string(&mint.url);
            let name = mint.name.as_ref().map(|value| fbb.create_string(value));
            let icon_url = mint.icon_url.as_ref().map(|value| fbb.create_string(value));
            let nuts = fbb.create_vector(&mint.nuts);
            let unit_offsets: Vec<_> = mint.units.iter().map(|unit| fbb.create_string(unit)).collect();
            let units = fbb.create_vector(&unit_offsets);
            let audit = mint.audit.as_ref().map(|summary| encode_audit(fbb, summary));

            fb::DiscoveredMintRow::create(
                fbb,
                &fb::DiscoveredMintRowArgs {
                    url: Some(url),
                    has_name: mint.name.is_some(),
                    name,
                    has_icon_url: mint.icon_url.is_some(),
                    icon_url,
                    nuts: Some(nuts),
                    units: Some(units),
                    supports_nutzap: mint.supports_nutzap,
                    trust_score: mint.trust_score,
                    recommendation_count: mint.recommendation_count,
                    via_fallback: mint.via_fallback,
                    has_audit: mint.audit.is_some(),
                    audit,
                },
            )
        })
        .collect();
    fbb.create_vector(&rows)
}

fn encode_audit<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    summary: &MintAuditSummary,
) -> WIPOffset<fb::MintAuditSummary<'a>> {
    let last_ok_at = summary
        .last_ok_at
        .as_ref()
        .map(|value| fbb.create_string(value));
    fb::MintAuditSummary::create(
        fbb,
        &fb::MintAuditSummaryArgs {
            rating: rating_to_fb(summary.rating),
            success_rate: summary.success_rate,
            sampled_swaps: summary.sampled_swaps,
            has_last_ok_at: summary.last_ok_at.is_some(),
            last_ok_at,
        },
    )
}

fn rating_to_fb(rating: MintAuditRating) -> fb::MintAuditRating {
    match rating {
        MintAuditRating::Unknown => fb::MintAuditRating::Unknown,
        MintAuditRating::Healthy => fb::MintAuditRating::Healthy,
        MintAuditRating::Degraded => fb::MintAuditRating::Degraded,
        MintAuditRating::Unreliable => fb::MintAuditRating::Unreliable,
    }
}

fn rating_from_fb(rating: fb::MintAuditRating) -> MintAuditRating {
    match rating {
        fb::MintAuditRating::Healthy => MintAuditRating::Healthy,
        fb::MintAuditRating::Degraded => MintAuditRating::Degraded,
        fb::MintAuditRating::Unreliable => MintAuditRating::Unreliable,
        _ => MintAuditRating::Unknown,
    }
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by
/// [`encode_mint_discovery_projection`]) back into a
/// [`MintDiscoveryProjection`]. Returns an error string on any malformed
/// input.
pub fn decode_mint_discovery_projection(bytes: &[u8]) -> Result<MintDiscoveryProjection, String> {
    if bytes.len() < 8 || !fb::mint_discovery_projection_buffer_has_identifier(bytes) {
        return Err("missing NMDS file identifier".to_string());
    }
    let root = fb::root_as_mint_discovery_projection(bytes)
        .map_err(|e| format!("not a valid MintDiscoveryProjection buffer: {e}"))?;

    let mints = decode_mints(root.mints())?;
    Ok(MintDiscoveryProjection { mints })
}

fn decode_mints(
    rows: Option<Vector<'_, ForwardsUOffset<fb::DiscoveredMintRow<'_>>>>,
) -> Result<Vec<DiscoveredMint>, String> {
    let Some(rows) = rows else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let name = if row.has_name() {
            Some(str_field(row.name(), "DiscoveredMintRow.name")?)
        } else {
            None
        };
        let icon_url = if row.has_icon_url() {
            Some(str_field(row.icon_url(), "DiscoveredMintRow.icon_url")?)
        } else {
            None
        };
        let nuts = row.nuts().map(|v| v.iter().collect()).unwrap_or_default();
        let units = row
            .units()
            .map(|v| {
                v.iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let audit = if row.has_audit() {
            Some(decode_audit(row.audit())?)
        } else {
            None
        };

        out.push(DiscoveredMint {
            url: str_field(row.url(), "DiscoveredMintRow.url")?,
            name,
            icon_url,
            nuts,
            units,
            supports_nutzap: row.supports_nutzap(),
            trust_score: row.trust_score(),
            recommendation_count: row.recommendation_count(),
            via_fallback: row.via_fallback(),
            audit,
        });
    }
    Ok(out)
}

fn decode_audit(table: Option<fb::MintAuditSummary<'_>>) -> Result<MintAuditSummary, String> {
    let Some(table) = table else {
        return Err("DiscoveredMintRow.audit missing despite has_audit".to_string());
    };
    let last_ok_at = if table.has_last_ok_at() {
        Some(str_field(
            table.last_ok_at(),
            "MintAuditSummary.last_ok_at",
        )?)
    } else {
        None
    };
    Ok(MintAuditSummary {
        rating: rating_from_fb(table.rating()),
        success_rate: table.success_rate(),
        sampled_swaps: table.sampled_swaps(),
        last_ok_at,
        // The auditor-captured icon URL rides on `DiscoveredMintRow.icon_url`
        // on the wire (see the schema doc), not duplicated on the nested
        // audit table — `apply_audit`/`enrich_with_audit` populate both
        // Rust-side fields from the one auditor lookup, but only
        // `icon_url` needs to cross the wire per mint.
        icon_url: None,
    })
}

fn str_field(value: Option<&str>, field: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{field} missing required string"))
}

#[cfg(test)]
#[path = "projection_wire_tests.rs"]
mod tests;
