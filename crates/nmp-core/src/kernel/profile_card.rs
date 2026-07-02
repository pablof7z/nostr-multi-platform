//! Profile summary card.
//!
//! Owns [`ProfileCard`], the kernel's raw kind:0 profile projection.
//!
//! ADR-0070 PR 2 context: the kernel-owned `Profile` struct + the
//! `profiles: HashMap<…>` field were deleted — the kind:0 profile cache is now
//! capability-owned (`nmp_nip01::ProfileCache` behind `Arc<dyn ProfileLookup>`,
//! the writer being the registered `nmp_nip01::Kind0Parser`). The kernel reads
//! cached profiles as `crate::substrate::ProfileView` through
//! `Kernel::profile_lookup()`; it no longer names the kind:0 wire format (D0).
//!
//! ADR-0070 Lane H context: `ProfileCard::from_mention()` and
//! `MentionProfilePayload` were deleted; the mention_profiles projection is
//! removed and profile data is served via the `refs.profile` KPRF NRRD
//! row-delta sidecar.

use super::Serialize;

/// Profile summary card.
///
/// Carries the raw kind:0 fields with `Option<String>` semantics — `None`
/// signals "no kind:0 has arrived yet for this field" so presentation
/// layers can choose their own fallback (typically formatting the raw
/// pubkey). aim.md §2 — NMP is a data framework; backend ships raw
/// protocol data, presentation layers own formatting.
#[derive(Clone, Debug, Serialize)]
pub(super) struct ProfileCard {
    pub(super) pubkey: String,
    // D6 / ADR-0072: `npub` (bech32) field removed — projection sends raw hex
    // pubkey only; shells encode bech32 host-side via UniFFI `encode_profile`
    // or their own implementation. Closes V-115.
    /// Display name from kind:0 (`display_name` / `displayName` / `name`,
    /// first non-empty wins). `None` when no kind:0 has arrived yet —
    /// presentation layer renders its own fallback.
    pub(super) display_name: Option<String>,
    /// Raw `name` field from kind:0. Kept distinct from derived
    /// `display_name` so hosts can edit one profile field without becoming a
    /// second kind:0 parser.
    pub(super) name: Option<String>,
    /// Raw snake-case `display_name` field from kind:0.
    pub(super) raw_display_name: Option<String>,
    /// Raw camel-case `displayName` field from kind:0.
    pub(super) display_name_camel: Option<String>,
    /// Picture URL from kind:0. `None` when no kind:0 has arrived yet
    /// or the metadata carries no `picture` field — presentation layer
    /// chooses a placeholder/identicon strategy.
    pub(super) picture_url: Option<String>,
    /// Raw `banner` field from kind:0.
    pub(super) banner: Option<String>,
    /// Raw `website` field from kind:0.
    pub(super) website: Option<String>,
    pub(super) nip05: String,
    pub(super) about: String,
    /// Raw `lud16` lightning address from kind:0.
    pub(super) lud16: Option<String>,
    /// Raw `lud06` LNURL field from kind:0.
    pub(super) lud06: Option<String>,
    /// Pre-extracted lightning address (`lud16`) / LNURL (`lud06`) from
    /// this pubkey's kind:0 metadata. `None` when no kind:0 has arrived
    /// or the user has no lightning address. The zap button in the shell
    /// is enabled/disabled based on this field — Rust decides
    /// zapability, the shell renders it.
    pub(super) lnurl: Option<String>,
}
