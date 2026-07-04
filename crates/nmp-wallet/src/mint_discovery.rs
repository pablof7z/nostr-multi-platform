//! NIP-87 mint discovery: web-of-trust-scoped, fail-closed aggregation of
//! kind:38172 announcements + kind:38000 recommendations into an app-facing
//! "discovered / recommended mints" view (issue #2880, epic #2864).
//!
//! Rust owns the discovery policy end to end; the shell only renders and
//! selects. Two invariants are load-bearing:
//!
//! - **Fail closed on capability.** A mint that does not advertise the NUTs
//!   required for NIP-61 nutzaps (NUT-11 P2PK + NUT-12 DLEQ, via
//!   [`nmp_nip87::MintCapabilities::supports_nutzap`]) is excluded from the
//!   recommended set — a wallet must never offer a mint it cannot safely lock
//!   or prove ecash on. A recommendation for a mint we have no announcement for
//!   is likewise dropped: with no advertised capabilities we cannot verify it,
//!   so we fail closed rather than surface an unvetted URL.
//! - **Trust is web-of-trust-scoped.** A recommendation only counts when its
//!   author passes the reading account's [`WotGraph`] policy (not hidden, score
//!   at or above the configured floor). The mint's rank is the sum of its
//!   distinct trusted recommenders' scores, so a mint vouched for by people the
//!   viewer follows sorts above one vouched for by strangers. The trust engine
//!   is reused from `nmp-wot`, not reinvented here.
//!
//! [`aggregate_discovered_mints`] is a pure function over already-decoded
//! inputs — the whole policy is unit-testable without a kernel. The
//! [`MintDiscoveryStore`] wraps it with the accumulation of observed events;
//! `discovery_runtime` wires that store to the kernel's read pipeline.

use std::collections::{BTreeMap, BTreeSet};

use nmp_nip87::{MintAnnouncement, MintCapabilities, MintRecommendation, NUTZAP_REQUIRED_NUTS};
use nmp_wot::WotGraph;
use serde::{Deserialize, Serialize};

use nmp_core::substrate::KernelEvent;
use nmp_wot::{KIND_CONTACT_LIST, KIND_MUTE_LIST};
use nmp_nip87::{KIND_MINT_ANNOUNCE, KIND_MINT_RECOMMEND};

/// Maximum discovered mints surfaced in the projection.
pub const MAX_DISCOVERED_MINTS: usize = 100;

/// Policy governing which mints qualify and which recommenders count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintDiscoveryPolicy {
    /// NUTs a mint must advertise to be surfaced. Defaults to the nutzap set
    /// (NUT-11 + NUT-12); a mint missing any of these fails closed.
    pub required_nuts: BTreeSet<u16>,
    /// Minimum web-of-trust score a recommender must have for their vouch to
    /// count. Defaults to `1`, so only recommenders the viewer has some trust
    /// path to (direct follows, follows-of-follows) contribute; strangers
    /// (score 0) and muted accounts are ignored.
    pub minimum_recommender_score: i32,
}

impl Default for MintDiscoveryPolicy {
    fn default() -> Self {
        Self {
            required_nuts: NUTZAP_REQUIRED_NUTS.into_iter().collect(),
            minimum_recommender_score: 1,
        }
    }
}

/// One discovered mint in the app-facing projection. Carries no ecash proofs,
/// keys, or other secret material — pure discovery metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct DiscoveredMint {
    /// Mint URL (the value a consumer selects instead of a hardcoded entry).
    pub url: String,
    /// Human-readable name, if any announcement advertised one.
    pub name: Option<String>,
    /// The addressable coordinate `38172:<author>:<d>` of an announcement for
    /// this mint (the first one seen), for callers that want to re-fetch it.
    pub announcement_coordinate: String,
    /// Supported NUT numbers (union across announcements for this URL).
    pub nuts: Vec<u16>,
    /// Supported units, when advertised.
    pub units: Vec<String>,
    /// True when the mint advertises the nutzap-required NUTs. Always `true`
    /// for mints in the default result set (they are the pass-filter), but
    /// carried explicitly so a caller using a looser `required_nuts` can still
    /// see the nutzap verdict.
    pub supports_nutzap: bool,
    /// Summed web-of-trust score of the distinct trusted recommenders.
    pub trust_score: i32,
    /// Count of distinct trusted recommenders.
    pub recommendation_count: u32,
}

/// The app-facing discovered-mints projection.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct MintDiscoveryProjection {
    /// Mints meeting the capability requirement, ranked by trust (descending),
    /// then recommendation count, then URL for deterministic ordering.
    pub mints: Vec<DiscoveredMint>,
}

/// Aggregate decoded announcements + recommendations into a ranked,
/// capability-filtered, web-of-trust-scoped discovered-mints view.
///
/// Pure: the entire discovery policy is exercised through this function.
#[must_use]
pub fn aggregate_discovered_mints(
    viewer: &str,
    announcements: &[MintAnnouncement],
    recommendations: &[MintRecommendation],
    wot: &WotGraph,
    policy: &MintDiscoveryPolicy,
) -> Vec<DiscoveredMint> {
    // 1. Index announcements by mint URL (merging capabilities across
    //    announcers) and by coordinate (so `a`-tag recommendations resolve to
    //    URLs).
    let mut by_url: BTreeMap<String, MintAccumulator> = BTreeMap::new();
    let mut coordinate_urls: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for announcement in announcements {
        let coordinate = announcement.coordinate();
        coordinate_urls
            .entry(coordinate.clone())
            .or_default()
            .extend(announcement.mint_urls.iter().cloned());
        for url in &announcement.mint_urls {
            let acc = by_url.entry(url.clone()).or_insert_with(|| MintAccumulator {
                coordinate: coordinate.clone(),
                ..MintAccumulator::default()
            });
            acc.merge_announcement(announcement);
        }
    }

    // 2. Fold in recommendations, web-of-trust-scoped, deduped per recommender.
    for recommendation in recommendations {
        let decision = wot.score(viewer, &recommendation.author);
        if decision.hide || decision.score < policy.minimum_recommender_score {
            continue;
        }
        let mut targets: BTreeSet<String> = recommendation.mint_urls.iter().cloned().collect();
        for coordinate in &recommendation.mint_coordinates {
            if let Some(urls) = coordinate_urls.get(coordinate) {
                targets.extend(urls.iter().cloned());
            }
        }
        for url in targets {
            // Fail closed: only count recommendations for mints we have an
            // announcement (hence advertised capabilities) for.
            if let Some(acc) = by_url.get_mut(&url) {
                if acc.recommenders.insert(recommendation.author.clone()) {
                    acc.trust_score = acc.trust_score.saturating_add(decision.score);
                }
            }
        }
    }

    // 3. Capability fail-closed filter + rank.
    let mut mints: Vec<DiscoveredMint> = by_url
        .into_iter()
        .filter_map(|(url, acc)| {
            if !acc.capabilities.supports_all(&policy.required_nuts) {
                return None;
            }
            Some(DiscoveredMint {
                url,
                name: acc.name,
                announcement_coordinate: acc.coordinate,
                nuts: acc.capabilities.nuts.iter().copied().collect(),
                units: acc.capabilities.units.iter().cloned().collect(),
                supports_nutzap: acc.capabilities.supports_nutzap(),
                trust_score: acc.trust_score,
                recommendation_count: acc.recommenders.len() as u32,
            })
        })
        .collect();

    mints.sort_by(|a, b| {
        b.trust_score
            .cmp(&a.trust_score)
            .then_with(|| b.recommendation_count.cmp(&a.recommendation_count))
            .then_with(|| a.url.cmp(&b.url))
    });
    mints.truncate(MAX_DISCOVERED_MINTS);
    mints
}

#[derive(Default)]
struct MintAccumulator {
    coordinate: String,
    name: Option<String>,
    capabilities: MintCapabilities,
    recommenders: BTreeSet<String>,
    trust_score: i32,
}

impl MintAccumulator {
    fn merge_announcement(&mut self, announcement: &MintAnnouncement) {
        if self.name.is_none() {
            self.name = announcement.name.clone();
        }
        self.capabilities
            .nuts
            .extend(announcement.capabilities.nuts.iter().copied());
        self.capabilities
            .units
            .extend(announcement.capabilities.units.iter().cloned());
    }
}

/// Accumulates observed NIP-87 events (and the viewer's follow/mute graph) and
/// produces the [`MintDiscoveryProjection`] on demand. Owns its own
/// [`WotGraph`] built from the same kind:3/kind:10000 events every other WoT
/// consumer reads — reusing `nmp-wot`'s scoring, not a second trust model.
#[derive(Default)]
pub struct MintDiscoveryStore {
    viewer: Option<String>,
    announcements: BTreeMap<String, (u64, MintAnnouncement)>,
    recommendations: BTreeMap<String, MintRecommendation>,
    wot: WotGraph,
    policy: MintDiscoveryPolicy,
}

impl MintDiscoveryStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the reading account whose web of trust scopes recommendations. A
    /// change clears nothing else — announcements/recommendations are
    /// account-independent public data; only the scoring viewer changes.
    pub fn set_viewer(&mut self, viewer: Option<String>) {
        self.viewer = viewer;
    }

    /// Ingest one observed kernel event. Non-discovery, non-graph kinds are
    /// ignored, so the same sink can be pointed at a coarse relay filter.
    pub fn ingest_kernel_event(&mut self, event: &KernelEvent) {
        match event.kind {
            KIND_MINT_ANNOUNCE => {
                if let Some(announcement) = nmp_nip87::decode_mint_announcement(
                    &event.id,
                    &event.author,
                    &event.tags,
                    &event.content,
                ) {
                    // Addressable replace-by-newer on the `(author, d)` coordinate.
                    let coordinate = announcement.coordinate();
                    let replace = self
                        .announcements
                        .get(&coordinate)
                        .is_none_or(|(seen_at, _)| event.created_at >= *seen_at);
                    if replace {
                        self.announcements
                            .insert(coordinate, (event.created_at, announcement));
                    }
                }
            }
            KIND_MINT_RECOMMEND => {
                if let Some(recommendation) = nmp_nip87::decode_mint_recommendation(
                    &event.id,
                    &event.author,
                    &event.tags,
                    &event.content,
                ) {
                    self.recommendations
                        .insert(recommendation.event_id.clone(), recommendation);
                }
            }
            KIND_CONTACT_LIST | KIND_MUTE_LIST => {
                self.wot.ingest_event(&event.author, event.kind, &event.tags);
            }
            _ => {}
        }
    }

    /// Compute the current discovered-mints projection. Empty until a viewer is
    /// set (recommendations cannot be trust-scored without one).
    #[must_use]
    pub fn snapshot(&self) -> MintDiscoveryProjection {
        let Some(viewer) = self.viewer.as_deref() else {
            return MintDiscoveryProjection::default();
        };
        let announcements: Vec<MintAnnouncement> = self
            .announcements
            .values()
            .map(|(_, announcement)| announcement.clone())
            .collect();
        let recommendations: Vec<MintRecommendation> =
            self.recommendations.values().cloned().collect();
        MintDiscoveryProjection {
            mints: aggregate_discovered_mints(
                viewer,
                &announcements,
                &recommendations,
                &self.wot,
                &self.policy,
            ),
        }
    }
}

#[cfg(test)]
#[path = "mint_discovery_tests.rs"]
mod tests;
