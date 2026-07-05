//! Optional Cashu-mint-auditor enrichment (`audit` feature).
//!
//! [`MintAuditSummary`] and [`apply_audit`] are always compiled — a plain,
//! dependency-free model plus a pure fold — so a caller can construct
//! summaries from any source (including tests) without the optional
//! `cashu-mint-audit` dependency. Only [`enrich_with_audit`] and the
//! `cashu_mint_audit`-specific conversions are gated behind the `audit`
//! feature, since they name that crate's types directly.
//!
//! # The hot-path boundary (doctrine, not a suggestion)
//!
//! [`enrich_with_audit`] performs real HTTP requests (via
//! `cashu_mint_audit::AuditorClient`, itself backed by `reqwest`). Per the
//! projections-and-emission doctrine, a registered snapshot-projection
//! closure runs on the actor thread inside `make_update` and MUST be
//! non-blocking — no I/O, no await (D8). **`enrich_with_audit` must never be
//! called from inside the closure passed to
//! `SnapshotProjectionRegistrar::register_typed_snapshot_projection`** (see
//! `register.rs` / `runtime.rs`). It is a composition-root-owned helper: the
//! app calls it on its own schedule (e.g. a periodic background task, or
//! once after `MintDiscoveryRuntime::snapshot()` returns a fresh mint list),
//! then feeds the resulting `(url, MintAuditSummary)` pairs to [`apply_audit`]
//! — or, more conveniently, calls `enrich_with_audit` directly on a
//! `Vec<DiscoveredMint>` it owns off the reactive path, then re-publishes
//! that enriched view however its own architecture does so. This crate's own
//! `MintDiscoveryStore`/`MintDiscoveryRuntime` never call it.

use serde::{Deserialize, Serialize};

use crate::discovery::DiscoveredMint;

/// A coarse reliability verdict, mirroring `cashu_mint_audit::HealthRating`
/// without naming that crate's type in the always-compiled model (so
/// `DiscoveredMint::audit` is usable with the `audit` feature off).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub enum MintAuditRating {
    /// No audit swaps in the sample — the auditor has no signal on this mint.
    #[default]
    Unknown,
    /// Nearly all recent swaps settled.
    Healthy,
    /// A meaningful minority of recent swaps failed.
    Degraded,
    /// Recent swaps failed often — treat with caution.
    Unreliable,
}

/// A per-mint reliability summary from the Cashu mint auditor
/// ([audit.8333.space](https://audit.8333.space)), applied by [`apply_audit`].
/// Carries no proofs, secrets, or keys — pure reliability metadata.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct MintAuditSummary {
    /// Coarse verdict derived from the sampled swap history.
    pub rating: MintAuditRating,
    /// `ok / sampled` in `0.0..=1.0`; `0.0` when the sample is empty.
    pub success_rate: f64,
    /// How many swaps were examined to produce this summary.
    pub sampled_swaps: u32,
    /// `created_at` of the most recent successful swap, if any (ISO-8601, as
    /// captured by the auditor).
    pub last_ok_at: Option<String>,
    /// Icon URL captured by the auditor from the mint's NUT-06 info, when
    /// present. NIP-87 announcements carry no icon field, so this is the only
    /// source [`DiscoveredMint::icon_url`] can be populated from.
    pub icon_url: Option<String>,
}

#[cfg(feature = "audit")]
impl From<cashu_mint_audit::HealthRating> for MintAuditRating {
    fn from(rating: cashu_mint_audit::HealthRating) -> Self {
        match rating {
            cashu_mint_audit::HealthRating::Unknown => Self::Unknown,
            cashu_mint_audit::HealthRating::Healthy => Self::Healthy,
            cashu_mint_audit::HealthRating::Degraded => Self::Degraded,
            cashu_mint_audit::HealthRating::Unreliable => Self::Unreliable,
        }
    }
}

/// Pure fold: apply auditor summaries (keyed by mint URL) onto matching
/// mints. A mint with no matching entry in `audits` is left unchanged
/// (`audit` stays whatever it already was — typically `None`). Also fills
/// `icon_url` from the summary when the mint had none yet, since the auditor
/// is the only source of that field (see [`MintAuditSummary::icon_url`]).
///
/// Unconditional on the `audit` feature: callers can exercise this fold with
/// hand-built summaries (e.g. in tests) without the optional dependency.
pub fn apply_audit(mints: &mut [DiscoveredMint], audits: &[(String, MintAuditSummary)]) {
    use std::collections::BTreeMap;

    let by_url: BTreeMap<&str, &MintAuditSummary> = audits
        .iter()
        .map(|(url, summary)| (url.as_str(), summary))
        .collect();

    for mint in mints.iter_mut() {
        if let Some(summary) = by_url.get(mint.url.as_str()) {
            if mint.icon_url.is_none() {
                mint.icon_url.clone_from(&summary.icon_url);
            }
            mint.audit = Some((*summary).clone());
        }
    }
}

/// Number of recent swaps sampled per mint by [`enrich_with_audit`].
#[cfg(feature = "audit")]
const AUDIT_SAMPLE_SWAPS: u32 = 50;

/// Fetch auditor health for each of `mints`' URLs and apply it via
/// [`apply_audit`]. Async, real network I/O — see the module-level "hot-path
/// boundary" doc: the composition root must call this OFF the reactive
/// projection-emit path, never inside a registered snapshot-projection
/// closure.
///
/// A mint the auditor has never seen (`AuditError::UnknownMint`) or a
/// transient auditor failure is skipped rather than surfaced as an error —
/// enrichment is best-effort reliability signal, not a correctness
/// dependency; a mint the auditor cannot answer for simply keeps `audit:
/// None`.
#[cfg(feature = "audit")]
pub async fn enrich_with_audit(
    mints: &mut [DiscoveredMint],
    client: &cashu_mint_audit::AuditorClient,
) {
    let mut audits: Vec<(String, MintAuditSummary)> = Vec::with_capacity(mints.len());
    for mint in mints.iter() {
        let Ok(audited) = client.mint_by_url(&mint.url).await else {
            continue;
        };
        let Ok(health) = client.health_for(audited.id, AUDIT_SAMPLE_SWAPS).await else {
            continue;
        };
        audits.push((
            mint.url.clone(),
            MintAuditSummary {
                rating: health.rating().into(),
                success_rate: health.success_rate,
                sampled_swaps: health.sampled_swaps as u32,
                last_ok_at: health.last_ok_at.clone(),
                icon_url: audited.icon_url(),
            },
        ));
    }
    apply_audit(mints, &audits);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mint(url: &str) -> DiscoveredMint {
        DiscoveredMint {
            url: url.to_string(),
            ..DiscoveredMint::default()
        }
    }

    #[test]
    fn apply_audit_sets_audit_and_backfills_icon_url_for_matching_mints() {
        let mut mints = vec![mint("https://a.mint"), mint("https://b.mint")];
        let summary = MintAuditSummary {
            rating: MintAuditRating::Healthy,
            success_rate: 0.99,
            sampled_swaps: 40,
            last_ok_at: Some("2026-07-04T00:00:00".to_string()),
            icon_url: Some("https://a.mint/icon.png".to_string()),
        };
        apply_audit(
            &mut mints,
            &[("https://a.mint".to_string(), summary.clone())],
        );

        assert_eq!(mints[0].audit, Some(summary));
        assert_eq!(mints[0].icon_url.as_deref(), Some("https://a.mint/icon.png"));
        assert_eq!(mints[1].audit, None, "no matching audit entry for b.mint");
        assert_eq!(mints[1].icon_url, None);
    }

    #[test]
    fn apply_audit_does_not_overwrite_an_existing_icon_url() {
        let mut mints = vec![DiscoveredMint {
            icon_url: Some("https://original.icon".to_string()),
            ..mint("https://a.mint")
        }];
        let summary = MintAuditSummary {
            icon_url: Some("https://auditor.icon".to_string()),
            ..MintAuditSummary::default()
        };
        apply_audit(&mut mints, &[("https://a.mint".to_string(), summary)]);
        assert_eq!(mints[0].icon_url.as_deref(), Some("https://original.icon"));
    }

    #[test]
    fn apply_audit_is_a_noop_over_an_empty_audit_list() {
        let mut mints = vec![mint("https://a.mint")];
        apply_audit(&mut mints, &[]);
        assert_eq!(mints[0].audit, None);
    }
}
