//! DX Login → Following-Timeline Gate — the empirical proof of `docs/aim.md`
//! §1's headline promise.
//!
//! `dx_scaffold_gate.rs` proves the *scaffold* compiles as a thin shell. It does
//! NOT prove the actual product claim. This gate closes that gap: it drives the
//! canonical worked example (`nmp-example-login-timeline`) through the REAL
//! kernel — login → open the following timeline → render rows from the
//! Rust-owned typed projection → receive a live event — and asserts a timeline
//! ROW actually renders, that a live event surfaces, and that the follow set is
//! load-bearing (it is a *following* timeline, not a global one).
//!
//! Every event is a real Schnorr-signed Nostr event routed through the kernel's
//! production ingest gate (verify → store → observer fan-out → OP-feed engine →
//! an app-owned NNFS typed projection). The shell decodes that projection with
//! the NMP-provided `decode_feed_row_snapshot` and renders rows. The shell writes
//! ZERO relay/cache/subscription/replaceable-policy code — proven structurally by
//! `g6_example_shell_is_doctrine_clean` (banned-substring scan of the example's
//! `lib.rs`), the same check `dx_scaffold_gate` G2/G4 apply to the scaffold.
//!
//! # Gates
//!
//! | Gate | Assertion                                                               |
//! |------|-------------------------------------------------------------------------|
//! | G1   | After login + follow, ≥1 following-timeline row renders (the followed   |
//! |      | author's note), decoded from the Rust-owned typed projection            |
//! | G2   | A live event from the followed author surfaces as a NEW row, no refresh |
//! | G3   | Follow set is load-bearing for ADMISSION: a followed author's note      |
//! |      | renders as its own row; a non-followed stranger's note (root-shaped OR  |
//! |      | reply-shaped) does NOT render at all. A feed that ignored the follow    |
//! |      | set (treated every author as followed — the "global feed" regression)   |
//! |      | would render the stranger's note, so this assertion FAILS on it         |
//! | G6   | The example's shell (`lib.rs`) has zero relay/cache/sub/replaceable LOC  |
//!
//! # Invocation
//!
//! ```sh
//! cargo test -p nmp-testing --test dx_login_timeline_gate
//! ```
//!
//! # Doctrine references
//! - `docs/aim.md` §1   — the one-shot login→timeline claim this proves
//! - `docs/aim.md` §2 inv-4 — No native business logic (G6)
//! - `docs/aim.md` §4.14 — scaffolding CLI contract (the example mirrors it)

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use nmp_example_login_timeline::harness::{
    contact_list, note, reply, run_demo, DemoApp, DEMO_NSEC,
};
use nostr::Keys;

// `NmpApp` spins process-global actor / listener threads; serialize the
// whole-lifecycle tests so exactly one app is live at a time (the established
// idiom across the `nmp_app_new`-based integration tests).
static SERIAL: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// G1 + G2: render a following-timeline row after login, then a live update
// ---------------------------------------------------------------------------

#[test]
fn g1_g2_login_renders_row_then_live_update_adds_row() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let result = run_demo();

    // G1 — at least one row rendered after login, and it is the followed
    // author's note, decoded out of the app-owned typed feed projection.
    assert!(
        !result.after_login.is_empty(),
        "G1 DX GAP: login → following-timeline rendered ZERO rows. The aim.md §1 \
         product claim (login → real timeline) is unproven."
    );
    let login_row = result
        .after_login
        .iter()
        .find(|r| r.author_pubkey == result.followed_author)
        .expect("G1 DX GAP: the followed author's note must be a rendered row");
    assert!(
        login_row.content.contains("first note"),
        "G1 DX GAP: rendered row content must be the followed author's note, got {:?}",
        login_row.content
    );

    // G2 — a live event from the followed author surfaced as a NEW row, with no
    // shell-side subscription / refresh work.
    let before = result.after_login.len();
    let after = result.after_live_update.len();
    assert!(
        after > before,
        "G2 DX GAP: a live note from the followed author did NOT add a row \
         (before={before}, after={after}). Live update is unproven."
    );
    assert!(
        result
            .after_live_update
            .iter()
            .any(|r| r.author_pubkey == result.followed_author && r.content.contains("live update")),
        "G2 DX GAP: the live note's content must appear in the rendered timeline"
    );
}

// ---------------------------------------------------------------------------
// G3: the follow set is load-bearing — this is a FOLLOWING timeline
// ---------------------------------------------------------------------------

/// Proves the follow set is load-bearing for feed ADMISSION — the one "is this
/// a *following* timeline?" axis this synthetic harness can observe.
///
/// The reply-rollup "attribution" concept (a non-followed root surfacing
/// because a FOLLOWED reply pointed at it, `Vec<attribution_pubkeys>` on the
/// row) was deleted along with the `RootIndexed` engine (#3082/#3086). Under
/// the current `FlatFeed` engine every admitted event is its own top-level
/// row (reply-rollup is no longer a framework behavior, #3082/#3092) and
/// admission for `source::active_user().follows()` is gated on
/// author-set membership: only an event from a followed author is delivered
/// to this session at all (the harness's injection seam still routes through
/// the session's live-shape-scoped observer — it bypasses the kernel's OUTER
/// `timeline_authors` relay-relevance gate, not this session's OWN author-set
/// admission).
///
/// So the decisive, FALSIFIABLE proof now is direct: a FOLLOWED author's note
/// (root-shaped or reply-shaped — shape no longer matters to admission)
/// renders as its own row; a NON-followed stranger's note (root-shaped or
/// reply-shaped) does not render AT ALL. A feed that ignored the follow set
/// (treated every author as followed — the "global feed" regression) would
/// render the stranger's notes too, so this assertion FAILS on it.
#[test]
fn g3_follow_set_is_load_bearing_for_admission() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let demo = DemoApp::login(DEMO_NSEC);
    let viewer_keys = Keys::parse(DEMO_NSEC).expect("valid demo nsec");

    let followed = Keys::generate(); // a FOLLOWED author
    let stranger = Keys::generate(); // a NON-followed author

    // Declare the follow set: the signed-in account follows `followed` (and,
    // via self-inclusion, itself) but never `stranger`.
    assert!(
        demo.ingest(&contact_list(&viewer_keys, 2_000, &[followed.public_key()])),
        "follow list must verify"
    );

    // A root-shaped note from the FOLLOWED author.
    let followed_root = note(&followed, 2_100, "a followed root note");
    let followed_root_id = followed_root.id.to_hex();
    assert!(demo.ingest(&followed_root), "followed root must verify");

    // A reply-shaped note from the SAME followed author. Shape no longer
    // matters to admission — this must render as its OWN row too, not fold
    // into any parent (there is no reply-rollup anymore).
    let followed_reply = reply(&followed, 2_200, &followed_root_id, "a followed reply");
    let followed_reply_id = followed_reply.id.to_hex();
    assert!(demo.ingest(&followed_reply), "followed reply must verify");

    // A root-shaped note from the NON-followed stranger.
    let stranger_root = note(&stranger, 2_300, "a stranger's root note");
    assert!(demo.ingest(&stranger_root), "stranger root must verify");

    // A reply-shaped note from the same NON-followed stranger, replying to
    // the followed author's root.
    let stranger_reply = reply(
        &stranger,
        2_400,
        &followed_root_id,
        "a stranger's reply, not followed",
    );
    assert!(demo.ingest(&stranger_reply), "stranger reply must verify");

    // Wait until BOTH followed rows have rendered.
    let rows = demo.rows_when(Duration::from_secs(5), |rows| {
        rows.iter().any(|r| {
            r.author_pubkey == followed.public_key().to_hex()
                && r.content.contains("a followed root note")
        }) && rows.iter().any(|r| {
            r.author_pubkey == followed.public_key().to_hex()
                && r.content.contains("a followed reply")
        })
    });

    assert!(
        rows.iter()
            .any(|r| r.author_pubkey == followed.public_key().to_hex()
                && r.content.contains("a followed root note")),
        "G3 DX GAP: a followed author's root-shaped note must render as its \
         own row. Rendered rows: {:?}",
        rows.iter()
            .map(|r| (r.author_pubkey.as_str(), r.content.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(
        rows.iter()
            .any(|r| r.author_pubkey == followed.public_key().to_hex()
                && r.content.contains("a followed reply")),
        "G3 DX GAP: a followed author's reply-shaped note must ALSO render as \
         its own row — there is no reply-rollup in the current model. \
         Rendered rows: {:?}",
        rows.iter()
            .map(|r| (r.author_pubkey.as_str(), r.content.as_str()))
            .collect::<Vec<_>>()
    );
    // Sanity: the two followed rows are genuinely distinct rows (own event
    // ids), not one row folding the other.
    assert_ne!(followed_root_id, followed_reply_id);

    // The decisive, FALSIFIABLE proof that the follow set is load-bearing:
    // NEITHER of the non-followed stranger's notes (root-shaped or
    // reply-shaped) may render, in any form. A feed that ignored the follow
    // set (a GLOBAL feed) would admit both.
    assert!(
        !rows
            .iter()
            .any(|r| r.author_pubkey == stranger.public_key().to_hex()
                || r.content.contains("stranger")),
        "G3 DX GAP: a NON-followed author's note surfaced as a timeline row — \
         the follow set is not gating admission (this would be a GLOBAL feed, \
         not a following timeline). Rendered rows: {:?}",
        rows.iter()
            .map(|r| (r.author_pubkey.as_str(), r.content.as_str()))
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// A sanity check that the canonical follow path (kind:3) also drives the feed
// ---------------------------------------------------------------------------

/// The `run_demo` path follows via kind:3; this asserts the contact-list write
/// path is honored by re-running the follow → note flow explicitly and checking
/// the followed author's note renders.
#[test]
fn g1b_kind3_follow_drives_following_timeline() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let demo = DemoApp::login(DEMO_NSEC);
    let viewer_keys = Keys::parse(DEMO_NSEC).expect("valid demo nsec");
    let author = Keys::generate();
    let author_hex = author.public_key().to_hex();

    // Declare the follow set with a signed kind:3 (the canonical write path).
    assert!(
        demo.ingest(&contact_list(&viewer_keys, 3_000, &[author.public_key()])),
        "kind:3 follow list must verify"
    );
    assert!(
        demo.ingest(&note(&author, 3_100, "note from a kind:3 follow")),
        "followed author's note must verify"
    );

    let rows = demo.rows_when(Duration::from_secs(5), |rows| {
        rows.iter().any(|r| r.author_pubkey == author_hex)
    });
    assert!(
        rows.iter()
            .any(|r| r.author_pubkey == author_hex && r.content.contains("kind:3 follow")),
        "G1b DX GAP: a kind:3-followed author's note must render in the following timeline"
    );
}

// ---------------------------------------------------------------------------
// G6: the example shell is doctrine-clean (zero framework-policy LOC)
// ---------------------------------------------------------------------------

fn example_lib_rs() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/nmp-testing; the example crate is a sibling.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .join("nmp-example-login-timeline")
        .join("src")
        .join("lib.rs")
}

#[test]
fn g5_example_uses_explicit_composition() {
    let content = std::fs::read_to_string(example_lib_rs()).expect("read example lib.rs");

    let mut cursor = 0;
    for installer in [
        "nmp_substrate::install",
        "nmp_nip50::register",
        "nmp_nip02::register",
        "nmp_replies::register",
        "nmp_nip25::register",
        "nmp_nip18::register",
        "nmp_nip84::register",
        "nmp_nip29::register",
        "nmp_wot::register",
        "nmp_nip51::register",
        "nmp_nip22::register",
        "nmp_nip17::register",
        "nmp_nip23::register",
    ] {
        let index = content[cursor..]
            .find(installer)
            .map(|offset| cursor + offset)
            .unwrap_or_else(|| {
                panic!(
                    "G5 DX GAP: login-timeline example must call owner installer `{installer}`.\n{content}"
                )
            });
        cursor = index + installer.len();
    }
    assert!(
        !content.contains("nmp_defaults") && !content.contains("nmp-defaults"),
        "G5 DX GAP: login-timeline example must not teach nmp-defaults.\n{content}",
    );
}

#[test]
fn g6_example_shell_is_doctrine_clean() {
    // The same framework-policy / business-logic patterns dx_scaffold_gate G2/G4
    // forbid in the generated scaffold. The example must obey the doctrine it
    // exists to demonstrate.
    let banned: &[&str] = &[
        "relay_pool",
        "add_relay(",
        "connect_relay",
        "select_relay",
        "relay_url",
        "cache_invalidat",
        "prune_cache",
        "subscribe(",
        "register_interest(",
        "replaceable",
    ];

    let content = std::fs::read_to_string(example_lib_rs()).expect("read example lib.rs");

    let mut hits = Vec::new();
    for (lineno, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // Skip doc-comments and code comments — only executable code counts.
        if trimmed.starts_with("//") {
            continue;
        }
        for pat in banned {
            if trimmed.contains(pat) {
                hits.push(format!("  line {}: {pat}: {line}", lineno + 1));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "G6 DX GAP: the example shell (lib.rs) contains {} line(s) of \
         relay/cache/subscription/replaceable-policy code that aim.md §1 says \
         the developer must NEVER touch.\n{}",
        hits.len(),
        hits.join("\n")
    );
}

#[test]
fn g7_example_uses_declared_feed_spec_path() {
    let content = std::fs::read_to_string(example_lib_rs()).expect("read example lib.rs");

    for required in [
        "FeedKey::app(",
        "feed::events()",
        "source::active_user().follows()",
        ".open_spec(",
        "FeedShape::Flat",
        "FeedOrder::NewestByFeedPosition",
        "FeedItemProjection::feed_rows()",
    ] {
        assert!(
            content.contains(required),
            "G7 DX GAP: login-timeline example must teach `{required}` as part \
             of the declared feed-spec path.\n{content}"
        );
    }

    for retired in [
        "open_active_follows_op_feed",
        "ProjectionKey::app_owned",
        "open_interest",
    ] {
        assert!(
            !content.contains(retired),
            "G7 DX GAP: login-timeline example must not teach retired feed path \
             `{retired}`.\n{content}"
        );
    }
}
