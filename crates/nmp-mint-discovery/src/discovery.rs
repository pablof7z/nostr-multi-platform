//! WoT-scoped, fail-closed aggregation of kind:38172 announcements +
//! kind:38000 recommendations into an app-facing "discovered / recommended
//! mints" view (issue #2880, epic #2864; extracted from `nmp-wallet` into
//! this installable crate so any Nostr app — not only a wallet — can compose
//! mint discovery).
//!
//! Rust owns the discovery policy end to end; the shell only renders and
//! selects. Three invariants are load-bearing:
//!
//! - **Fail closed on capability.** A mint that does not advertise the NUTs
//!   required for NIP-61 nutzaps (NUT-11 P2PK + NUT-12 DLEQ, via
//!   [`nmp_nip87::MintCapabilities::supports_nutzap`]) is excluded from the
//!   recommended set — a wallet must never offer a mint it cannot safely lock
//!   or prove ecash on. A recommendation for a mint we have no announcement
//!   for is likewise dropped: with no advertised capabilities we cannot
//!   verify it, so we fail closed rather than surface an unvetted URL.
//! - **Trust is web-of-trust-scoped, with an opt-in cold-start seed.** A
//!   recommendation only counts when its author passes
//!   [`WotGraph::score_rooted_with_minimum_score`] — the reading account's own
//!   graph when it has one, or [`DiscoveryPolicy::fallback_root`] when it is
//!   cold (no ingested follows). The mint's rank is the sum of its distinct
//!   trusted recommenders' scores, so a mint vouched for by people the viewer
//!   follows sorts above one vouched for by strangers. The trust engine is
//!   reused from `nmp-wot`, not reinvented here.
//! - **A rerouted (fallback-root) verdict is labeled, not hidden.** When
//!   scoring actually rerouted through `fallback_root` (the viewer had no
//!   follows of their own), every mint that counted a recommendation scored
//!   that way carries [`DiscoveredMint::via_fallback`] so a shell can render
//!   "suggested" rather than "trusted by people you follow".
//!
//! [`aggregate`] is a pure function over already-decoded inputs — the whole
//! policy is unit-testable without a kernel. The [`MintDiscoveryStore`] wraps
//! it with the accumulation of observed events; `runtime` wires that store to
//! the kernel's read pipeline.

use std::collections::{BTreeMap, BTreeSet};

use nmp_nip87::{MintAnnouncement, MintCapabilities, MintRecommendation, NUTZAP_REQUIRED_NUTS};
use nmp_wot::WotGraph;
use serde::{Deserialize, Serialize};

use nmp_core::substrate::KernelEvent;
use nmp_nip87::{KIND_MINT_ANNOUNCE, KIND_MINT_RECOMMEND};
use nmp_wot::{KIND_CONTACT_LIST, KIND_MUTE_LIST};

/// A Nostr public key, hex-encoded. Local alias (mirrors the same
/// per-crate convention `nmp-planner`/`nmp-nip01` already use) rather than a
/// shared type — this crate has no reason to depend on another crate just for
/// a string newtype.
pub type Pubkey = String;

/// Default cap on discovered mints surfaced in the projection, and
/// [`DiscoveryPolicy::default`]'s `max_results`.
pub const MAX_DISCOVERED_MINTS: usize = 100;

/// Policy governing which mints qualify and which recommenders count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryPolicy {
    /// NUTs a mint must advertise to be surfaced. Defaults to the nutzap set
    /// (NUT-11 + NUT-12); a mint missing any of these fails closed.
    pub required_nuts: BTreeSet<u16>,
    /// Minimum web-of-trust score a recommender must have for their vouch to
    /// count. Defaults to `1`, so only recommenders the effective root has
    /// some trust path to (direct follows, follows-of-follows) contribute;
    /// strangers (score 0) and muted accounts are ignored.
    pub minimum_recommender_score: i32,
    /// Caller-owned cold-start trust seed (e.g. an app's curated pubkey, or a
    /// well-known community root). When the viewer has no ingested follows of
    /// their own, scoring reroutes through this pubkey's already-ingested
    /// graph instead of scoring everything at 0 (see
    /// [`nmp_wot::WotGraph::score_rooted`]). `None` reproduces the
    /// no-fallback behavior exactly: a cold viewer sees no trusted mints.
    pub fallback_root: Option<Pubkey>,
    /// Maximum discovered mints returned by [`aggregate`], best-ranked first.
    pub max_results: usize,
}

impl Default for DiscoveryPolicy {
    fn default() -> Self {
        Self {
            required_nuts: NUTZAP_REQUIRED_NUTS.into_iter().collect(),
            minimum_recommender_score: 1,
            fallback_root: None,
            max_results: MAX_DISCOVERED_MINTS,
        }
    }
}

/// One discovered mint in the app-facing projection. Carries no ecash proofs,
/// keys, or other secret material — pure discovery metadata.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct DiscoveredMint {
    /// Mint URL (the value a consumer selects instead of a hardcoded entry).
    pub url: String,
    /// Human-readable name, if any announcement advertised one.
    pub name: Option<String>,
    /// Icon URL, when known. NIP-87 announcements carry no icon field; this
    /// is populated only by the optional `audit` feature's enrichment (from
    /// the auditor's captured NUT-06 mint info) — always `None` otherwise.
    pub icon_url: Option<String>,
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
    /// True when this mint's trust score was computed by rerouting through
    /// [`DiscoveryPolicy::fallback_root`] (the viewer had no follows of their
    /// own) rather than the viewer's own graph. See
    /// [`nmp_wot::TrustDecision::rooted_at_fallback`].
    pub via_fallback: bool,
    /// Cashu-mint-auditor reliability summary, when the optional `audit`
    /// feature enriched this mint (see the `audit` module). `None` when the
    /// feature is disabled, the auditor has no data for this mint, or
    /// enrichment has not run yet.
    pub audit: Option<crate::audit::MintAuditSummary>,
}

/// The app-facing discovered-mints projection.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
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
pub fn aggregate(
    viewer: &str,
    announcements: &[MintAnnouncement],
    recommendations: &[MintRecommendation],
    wot: &WotGraph,
    policy: &DiscoveryPolicy,
) -> Vec<DiscoveredMint> {
    // 1. Index announcements by mint URL (merging capabilities across
    //    announcers) and by coordinate (so `a`-tag recommendations resolve to
    //    URLs).
    let mut by_url: BTreeMap<String, MintAccumulator> = BTreeMap::new();
    let mut coordinate_urls: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for announcement in announcements {
        let coordinate = announcement.coordinate();
        coordinate_urls
            .entry(coordinate)
            .or_default()
            .extend(announcement.mint_urls.iter().cloned());
        for url in &announcement.mint_urls {
            let acc = by_url.entry(url.clone()).or_default();
            acc.merge_announcement(announcement);
        }
    }

    // 2. Fold in recommendations, web-of-trust-scoped (with the policy's
    //    cold-start fallback root), deduped per recommender.
    for recommendation in recommendations {
        let decision = wot.score_rooted_with_minimum_score(
            viewer,
            policy.fallback_root.as_deref(),
            &recommendation.author,
            policy.minimum_recommender_score,
        );
        if decision.hide {
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
                    acc.via_fallback |= decision.rooted_at_fallback;
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
                icon_url: None,
                nuts: acc.capabilities.nuts.iter().copied().collect(),
                units: acc.capabilities.units.iter().cloned().collect(),
                supports_nutzap: acc.capabilities.supports_nutzap(),
                trust_score: acc.trust_score,
                recommendation_count: acc.recommenders.len() as u32,
                via_fallback: acc.via_fallback,
                audit: None,
            })
        })
        .collect();

    mints.sort_by(|a, b| {
        b.trust_score
            .cmp(&a.trust_score)
            .then_with(|| b.recommendation_count.cmp(&a.recommendation_count))
            .then_with(|| a.url.cmp(&b.url))
    });
    mints.truncate(policy.max_results);
    mints
}

#[derive(Default)]
struct MintAccumulator {
    name: Option<String>,
    capabilities: MintCapabilities,
    recommenders: BTreeSet<String>,
    trust_score: i32,
    via_fallback: bool,
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
///
/// # Memoized snapshot (hot-path safety, #2880 review follow-up)
///
/// [`Self::snapshot`] runs on this crate's typed-projection emit path (up to
/// `DEFAULT_EMIT_HZ` = 4 Hz under relay churn), and a full [`aggregate`] there
/// is unbounded work (clone every announcement + recommendation, rebuild a
/// `BTreeMap`, run `nmp-wot` scoring, sort). That violates the
/// projections-and-emission doctrine ("closures MUST read pre-computed engine
/// state; MUST NOT allocate in steady state"). So the store memoizes:
/// [`Self::snapshot`] serves a cached [`MintDiscoveryProjection`] in
/// O(result) (a bounded `≤ policy.max_results` clone) and only re-aggregates
/// when [`Self::cached`] is `None`. EVERY mutating path (`set_viewer` when the
/// viewer actually changes, `ingest_kernel_event` when it actually stores an
/// announcement, a recommendation, or a follow/mute-list update) invalidates
/// the cache so a stale projection is never served — see each path's
/// `invalidate()` call.
#[derive(Default)]
pub struct MintDiscoveryStore {
    viewer: Option<String>,
    announcements: BTreeMap<String, (u64, MintAnnouncement)>,
    recommendations: BTreeMap<String, MintRecommendation>,
    wot: WotGraph,
    policy: DiscoveryPolicy,
    /// Memoized projection. `None` == dirty (inputs changed since the last
    /// compute, or nothing computed yet); `Some` == the current value, safe to
    /// clone out in O(result). Cleared by [`Self::invalidate`] on every actual
    /// mutation. Not part of the store's logical identity, so it is excluded
    /// from any equality reasoning.
    cached: Option<MintDiscoveryProjection>,
    /// Test-only count of full [`Self::compute`] runs, so memoization tests can
    /// prove a clean `snapshot` serves the cache (no recompute) and a mutation
    /// forces exactly one recompute. Never present in a release build.
    #[cfg(test)]
    compute_count: usize,
}

impl MintDiscoveryStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a store with a non-default [`DiscoveryPolicy`] (e.g. a
    /// caller-supplied `fallback_root`).
    #[must_use]
    pub fn with_policy(policy: DiscoveryPolicy) -> Self {
        Self {
            policy,
            ..Self::default()
        }
    }

    /// Mark the memoized snapshot dirty. Called by every path that mutates an
    /// input [`aggregate`] reads (announcements, recommendations, the WoT
    /// graph, or the scoring viewer).
    fn invalidate(&mut self) {
        self.cached = None;
    }

    /// Set the reading account whose web of trust scopes recommendations. A
    /// change clears nothing else — announcements/recommendations are
    /// account-independent public data; only the scoring viewer changes. A
    /// no-op set (same viewer) leaves the memoized snapshot intact.
    pub fn set_viewer(&mut self, viewer: Option<String>) {
        if self.viewer != viewer {
            self.viewer = viewer;
            self.invalidate();
        }
    }

    /// Ingest one observed kernel event. Non-discovery, non-graph kinds are
    /// ignored, so the same sink can be pointed at a coarse relay filter. Any
    /// branch that actually stores state invalidates the memoized snapshot.
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
                        self.invalidate();
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
                    self.invalidate();
                }
            }
            KIND_CONTACT_LIST | KIND_MUTE_LIST => {
                self.wot.ingest_event(&event.author, event.kind, &event.tags);
                self.invalidate();
            }
            _ => {}
        }
    }

    /// The current discovered-mints projection, memoized (see the type-level
    /// docs). Serves the cached value in O(result) when clean; re-runs
    /// [`aggregate`] and re-caches only when dirty. Empty until a viewer is
    /// set (recommendations cannot be trust-scored without one), unless a
    /// `fallback_root` policy is configured to still score against the seed.
    #[must_use]
    pub fn snapshot(&mut self) -> MintDiscoveryProjection {
        if self.cached.is_none() {
            let projection = self.compute();
            #[cfg(test)]
            {
                self.compute_count += 1;
            }
            self.cached = Some(projection);
        }
        // The `is_none` guard above guarantees `Some`.
        self.cached.clone().unwrap_or_default()
    }

    /// Re-aggregate the discovered-mints projection from current inputs. The
    /// unbounded work `snapshot`'s memoization keeps off the steady-state emit
    /// path.
    #[must_use]
    fn compute(&self) -> MintDiscoveryProjection {
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
            mints: aggregate(
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
#[path = "discovery_tests.rs"]
mod tests;
