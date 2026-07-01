//! `nmp-example-login-timeline` — the canonical DX worked example that proves
//! `docs/aim.md` §1's one-shot claim end to end:
//!
//! > a developer should be able to one-shot a working Nostr application —
//! > login, timeline … without ever touching relay routing, cache
//! > invalidation, replaceable-event semantics, or subscription lifecycle.
//!
//! This crate is the **thin shell** half of that proof: the entire app-author
//! surface needed to go from *no account* to *a rendered following timeline
//! that updates live* is the three functions below. Every one of them is either
//! a single framework composition call or pure presentation. There is NOT ONE
//! line of relay routing, cache invalidation, replaceable-event policy, or
//! subscription-lifecycle code here — all of that lives in NMP, behind the
//! `NmpAppBuilder` / `NmpApp` seam.
//!
//! The headless host that drives login + seeds events + renders to stdout lives
//! in the [`harness`] module (feature-gated, test-support only) and in
//! `examples/login_timeline.rs`. The CI proof lives in
//! `crates/nmp-testing/tests/dx_login_timeline_gate.rs`. Keeping the seeding /
//! relay / injection machinery OUT of this file is deliberate: this file is the
//! artifact the doctrine lint and the dx-gate banned-substring checks run
//! against, so it must itself obey the doctrine it exists to demonstrate.
//!
//! # The three-function shell
//!
//! 1. [`register`] — install explicit named substrate/protocol features. This
//!    mirrors the `nmp init` scaffold's ADR-0069 production composition path.
//! 2. [`register_following_timeline`] — open the FOLLOWING timeline. One call
//!    to [`nmp_native_runtime::register_op_feed_defaults`] wires the OP-centric home
//!    feed (the following timeline): ingest fan-out, the live follow-set
//!    predicate, the seq-ordered pull pager, and this app's typed projection.
//!    The shell never names a relay, a filter, or a subscription.
//! 3. [`render_home_rows`] — read the Rust-owned typed projection and turn it
//!    into renderable rows. Pure presentation: decode the FlatBuffers sidecar,
//!    copy raw protocol fields, format a short pubkey. No policy, no caching,
//!    no derivation.
//!
//! A real iOS/Android/desktop shell substitutes its own renderer for
//! [`render_home_rows`] (SwiftUI `List`, Compose `LazyColumn`, egui) over the
//! identical typed projection — the projection→render contract is
//! platform-agnostic.

use nmp_core::substrate::{ActionRegistrar, AppHost};
use nmp_feed::ProjectionKey;
use nmp_native_runtime::NmpApp;
use nmp_note_feed::op_feed::decode_op_feed_snapshot;

#[cfg(feature = "harness")]
pub mod harness;
pub mod private_status;

/// The primary note kinds the following timeline renders: kind:1 text notes.
/// Repost wrappers are derived below this app-facing declaration.
pub const FOLLOWING_PRIMARY_FEED_KINDS: [u32; 1] = [1];
pub const FOLLOWING_TIMELINE_PROJECTION_KEY: &str = "example.login_timeline.following";

/// Step 1 — install the tutorial composition explicitly.
///
/// This worked example mirrors the ADR-0069 starter path: install reusable
/// substrate/protocol features by name, then let the timeline opener register
/// the app-facing feed session. Call before `start`.
pub fn register(app: &mut (impl AppHost + ActionRegistrar)) {
    let _substrate = nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());

    nmp_nip50::register_search_scopes(app);
    nmp_nip50::register_input_scopes(app);

    nmp_nip02::register_follow_actions(app);
    nmp_replies::register_actions(app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip18::Nip18Descriptor, app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip84::Nip84Descriptor, app);
    nmp_nip29::register_input_scopes(app);

    let _wot = nmp_wot::register_runtime(app);
    let _mute = nmp_nip51::register_mute_runtime(app);
    let _bookmarks = nmp_nip51::register_bookmark_runtime(app);
    nmp_nip51::register_bookmark_set_runtime(app);
    nmp_nip51::register_web_bookmark_runtime(app);
    let _search_relays = nmp_nip51::register_search_relay_runtime_with_fallbacks(
        app,
        nmp_nip50::SearchFallbackRelays::default(),
    );
    let _comments = nmp_nip22::register_runtime(app);

    nmp_nip17::register_actions(app);
    nmp_nip17::register_runtime(app);

    nmp_content::register_longform_projection(app);
    ActionRegistrar::register_action(app, private_status::PublishStatusModule)
        .expect("starter private status action namespace must be unique");
}

/// Step 2 — open the FOLLOWING timeline.
///
/// One framework call wires the OP-centric following feed under this example's
/// app-owned projection key: the
/// engine is registered as a declared observed projection (ingest) AND as the
/// feed controller + typed projection (output). The follow-set predicate is read
/// LIVE from the active account's contact list, so once the user signs in and
/// their kind:3 is known, the timeline shows exactly their follows' notes — the
/// shell does not select relays, build filters, open subscriptions, or
/// invalidate caches for any of it.
///
/// `viewer_pubkey_hex` is the signed-in account's hex pubkey (used by the engine
/// for self-attribution); the live follow set is read from the kernel's
/// active-account slot regardless, so this is advisory.
pub fn register_following_timeline(app: &NmpApp, viewer_pubkey_hex: impl Into<String>) {
    let _defaults = nmp_native_runtime::register_op_feed_defaults(
        app,
        viewer_pubkey_hex.into(),
        FOLLOWING_PRIMARY_FEED_KINDS.to_vec(),
        ProjectionKey(FOLLOWING_TIMELINE_PROJECTION_KEY.to_string()),
    );
}

/// One renderable row of the following timeline.
///
/// Raw protocol data only (aim.md §2): the presentation layer decides how to
/// truncate the pubkey, format the timestamp, and lay out the content. The shell
/// carries no denormalized display copy and makes no fallback decisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineRow {
    /// Root note author, hex pubkey (64 chars).
    pub author_pubkey: String,
    /// Root note publish time, Unix seconds.
    pub created_at: u64,
    /// Verbatim note body.
    pub content: String,
    /// Hex pubkeys of followed authors who replied in this root's thread
    /// (NIP-10 attribution). Empty for a plain note. This list is populated by
    /// the kernel's follow-set predicate — its contents are the proof that this
    /// is a *following* timeline, not a global one.
    pub attribution_pubkeys: Vec<String>,
}

impl TimelineRow {
    /// Format the row for a text shell (TUI / stdout). A presentation helper —
    /// short-pubkey truncation and layout are render decisions that legitimately
    /// live in the shell (aim.md §2: `short_npub`-class helpers are fine in
    /// render code, never in projection builders).
    #[must_use]
    pub fn render_line(&self) -> String {
        let who = short_pubkey(&self.author_pubkey);
        let mut line = format!("{who}  {}", self.content);
        if !self.attribution_pubkeys.is_empty() {
            let names: Vec<String> = self
                .attribution_pubkeys
                .iter()
                .map(|p| short_pubkey(p))
                .collect();
            line.push_str(&format!("   [reply in thread by {}]", names.join(", ")));
        }
        line
    }
}

/// Step 3 — render the FOLLOWING timeline from the Rust-owned typed projection.
///
/// Reads this example's typed FlatBuffers sidecar (`NNFS`) the kernel emits
/// every tick, decodes it with the NMP-provided [`decode_op_feed_snapshot`], and
/// maps each root card to a [`TimelineRow`]. This is exactly the projection→
/// render contract a platform shell follows (iOS's `TypedHomeFeedDecoder` does
/// the same decode over the same bytes).
///
/// Returns an empty vec before any note is ingested or if the projection is
/// absent — the shell renders an empty list, never an error (aim.md doctrine 8:
/// no errors cross the seam).
#[must_use]
pub fn render_home_rows(app: &NmpApp) -> Vec<TimelineRow> {
    let typed = app.run_typed_snapshot_projections();
    let Some(home) = typed
        .into_iter()
        .find(|t| t.key == FOLLOWING_TIMELINE_PROJECTION_KEY)
    else {
        return Vec::new();
    };
    let Ok(snapshot) = decode_op_feed_snapshot(&home.payload) else {
        return Vec::new();
    };
    snapshot
        .cards
        .iter()
        .map(|root| TimelineRow {
            author_pubkey: root.card.author_pubkey.clone(),
            created_at: root.card.created_at,
            content: root.card.content.clone(),
            attribution_pubkeys: root
                .attribution
                .iter()
                .map(|a| a.author_pubkey.clone())
                .collect(),
        })
        .collect()
}

/// Truncate a 64-char hex pubkey to a short, render-friendly form. A pure
/// presentation helper (aim.md §2 permits these in render code).
#[must_use]
pub fn short_pubkey(hex: &str) -> String {
    if hex.len() <= 12 {
        return hex.to_string();
    }
    format!("{}...{}", &hex[..8], &hex[hex.len() - 4..])
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
