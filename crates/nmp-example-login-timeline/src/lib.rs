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
//!    through `app.feeds().open_spec(...)` submits this app's intent-level
//!    declaration: primary kind:1 rows from the active account's live follow
//!    set, projected as a root-indexed feed under this app-owned key. The shell
//!    never names a relay, a filter, or a subscription.
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
use nmp_feed::{
    feed, source, FeedHandle, FeedItemProjection, FeedKey, FeedOrder, FeedShape, FeedSpec,
    FeedWindowPolicy,
};
use nmp_native_runtime::NmpApp;
use nmp_feed::typed_wire::decode_feed_row_snapshot;

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

    let _nip50 = nmp_nip50::register(app, nmp_nip50::Config::default())
        .expect("nmp-nip50 registration must not collide");
    let _nip02 = nmp_nip02::register(app, nmp_nip02::Config::default())
        .expect("nmp-nip02 registration must not collide");
    let _replies = nmp_replies::register(app, nmp_replies::Config::default())
        .expect("nmp-replies registration must not collide");
    let _nip25 = nmp_nip25::register(app, nmp_nip25::Config::default())
        .expect("nmp-nip25 registration must not collide");
    let _nip18 = nmp_nip18::register(app, nmp_nip18::Config::default())
        .expect("nmp-nip18 registration must not collide");
    let _nip84 = nmp_nip84::register(app, nmp_nip84::Config::default())
        .expect("nmp-nip84 registration must not collide");
    let _nip29 = nmp_nip29::register(app, nmp_nip29::Config::default())
        .expect("nmp-nip29 registration must not collide");
    let _wot = nmp_wot::register(app, nmp_wot::Config::default())
        .expect("nmp-wot registration must not collide");
    let _nip51 = nmp_nip51::register(
        app,
        nmp_nip51::Config {
            search_fallback_relays: nmp_nip50::SearchFallbackRelays::default(),
        },
    )
    .expect("nmp-nip51 registration must not collide");
    let _comments = nmp_nip22::register(app, nmp_nip22::Config::default())
        .expect("nmp-nip22 registration must not collide");
    let _nip17 = nmp_nip17::register(app, nmp_nip17::Config::default())
        .expect("nmp-nip17 registration must not collide");
    let _nip23 = nmp_nip23::register(app, nmp_nip23::Config::default())
        .expect("nmp-nip23 registration must not collide");
    ActionRegistrar::register_action(app, private_status::PublishStatusModule)
        .expect("starter private status action namespace must be unique");
}

/// Step 2 — open the FOLLOWING timeline.
///
/// One framework call submits the typed feed declaration under this example's
/// app-owned feed key. NMP compiles that declaration into live active-account
/// follow acquisition, root-indexed feed projection, paging, and teardown. The
/// shell does not select relays, build filters, open subscriptions, or
/// invalidate caches for any of it.
///
/// The returned handle is the only lifecycle token for paging/close. The
/// example harness stores it and closes by handle, matching the app-facing feed
/// API a native shell would use.
pub fn register_following_timeline(
    app: &NmpApp,
) -> Result<FeedHandle, nmp_native_runtime::FeedSpecOpenError> {
    app.feeds()
        .open_spec(following_timeline_key(), following_timeline_spec())
}

/// App-owned feed output key for the worked example.
#[must_use]
pub fn following_timeline_key() -> FeedKey {
    FeedKey::app(FOLLOWING_TIMELINE_PROJECTION_KEY)
        .expect("example following timeline key must be app-owned")
}

/// Intent-level declaration for the worked example's following timeline.
#[must_use]
pub fn following_timeline_spec() -> FeedSpec {
    feed::events()
        .primary_kinds(FOLLOWING_PRIMARY_FEED_KINDS)
        .from(source::active_user().follows())
        .shape(FeedShape::Flat)
        .order(FeedOrder::NewestByFeedPosition)
        .window(FeedWindowPolicy::bounded(
            nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        ))
        .project(FeedItemProjection::feed_rows())
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
}

impl TimelineRow {
    /// Format the row for a text shell (TUI / stdout). A presentation helper —
    /// short-pubkey truncation and layout are render decisions that legitimately
    /// live in the shell (aim.md §2: `short_npub`-class helpers are fine in
    /// render code, never in projection builders).
    #[must_use]
    pub fn render_line(&self) -> String {
        let who = short_pubkey(&self.author_pubkey);
        format!("{who}  {}", self.content)
    }
}

/// Step 3 — render the FOLLOWING timeline from the Rust-owned typed projection.
///
/// Reads this example's typed FlatBuffers sidecar (`NNFS`) the kernel emits
/// every tick, decodes it with the NMP-provided [`decode_feed_row_snapshot`], and
/// maps each root card to a [`TimelineRow`]. This is exactly the projection→
/// render contract a platform shell follows: decode the NNFS bytes and render
/// rows from the same typed projection.
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
    let Ok(snapshot) = decode_feed_row_snapshot(&home.payload) else {
        return Vec::new();
    };
    snapshot
        .cards
        .iter()
        .map(|root| TimelineRow {
            author_pubkey: root.card.author_pubkey.clone(),
            created_at: root.card.created_at,
            content: root.card.content.clone(),
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
