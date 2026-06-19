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
//! 1. [`register`] — inherit the canonical NMP composition (mirrors the
//!    `nmp init` scaffold's `register`; one call to
//!    [`nmp_defaults::register_defaults`]).
//! 2. [`register_following_timeline`] — open the FOLLOWING timeline. One call
//!    to [`nmp_defaults::register_op_feed_defaults`] wires the OP-centric home
//!    feed (the following timeline): ingest fan-out, the live follow-set
//!    predicate, the seq-ordered pull pager, and the typed `nmp.feed.home`
//!    projection. The shell never names a relay, a filter, or a subscription.
//! 3. [`render_home_rows`] — read the Rust-owned typed projection and turn it
//!    into renderable rows. Pure presentation: decode the FlatBuffers sidecar,
//!    copy raw protocol fields, format a short pubkey. No policy, no caching,
//!    no derivation.
//!
//! A real iOS/Android/desktop shell substitutes its own renderer for
//! [`render_home_rows`] (SwiftUI `List`, Compose `LazyColumn`, egui) over the
//! identical `nmp.feed.home` typed projection — the projection→render contract
//! is platform-agnostic.

use nmp_core::substrate::AppHost;
use nmp_ffi::NmpApp;
use nmp_nip01::op_feed::{decode_op_feed_snapshot, OP_FEED_SNAPSHOT_KEY};

#[cfg(feature = "harness")]
pub mod harness;

/// The note kinds the following timeline subscribes to: kind:1 text notes and
/// kind:6 reposts (NIP-18). This is the only protocol detail the example
/// declares, and it is a *capability selection* (which note kinds this app
/// renders), not relay/subscription policy — the kernel turns it into the
/// follow-feed subscription shape and routes it. Matches Chirp's home feed.
pub const FOLLOWING_FEED_KINDS: [u32; 2] = [1, 6];

/// Step 1 — inherit the canonical NMP composition.
///
/// Identical in spirit to the `nmp init` scaffold's `register`: one call to
/// [`nmp_defaults::register_defaults`] wires the NIP-01/02/17/57/65 action
/// modules, the production routing substrate, and the standard runtime
/// controllers. Call before `start`.
pub fn register(app: &mut impl AppHost) {
    nmp_defaults::register_defaults(app);
}

/// Step 2 — open the FOLLOWING timeline.
///
/// One framework call wires the OP-centric home feed (`nmp.feed.home`): the
/// engine is registered as a kernel event observer (ingest) AND as the feed
/// controller + typed projection (output). The follow-set predicate is read
/// LIVE from the active account's contact list, so once the user signs in and
/// their kind:3 is known, the timeline shows exactly their follows' notes — the
/// shell does not select relays, build filters, open subscriptions, or
/// invalidate caches for any of it.
///
/// `viewer_pubkey_hex` is the signed-in account's hex pubkey (used by the engine
/// for self-attribution); the live follow set is read from the kernel's
/// active-account slot regardless, so this is advisory.
pub fn register_following_timeline(app: &NmpApp, viewer_pubkey_hex: impl Into<String>) {
    let _defaults = nmp_defaults::register_op_feed_defaults(
        app,
        viewer_pubkey_hex.into(),
        FOLLOWING_FEED_KINDS.to_vec(),
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
/// Reads the `nmp.feed.home` typed FlatBuffers sidecar (`NOFS`) the kernel emits
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
    let Some(home) = typed.into_iter().find(|t| t.key == OP_FEED_SNAPSHOT_KEY) else {
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
