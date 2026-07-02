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
//! the NMP-provided `decode_op_feed_snapshot` and renders rows. The shell writes
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
//! | G3   | Follow set is load-bearing for ATTRIBUTION: a followed (self) reply     |
//! |      | attributes to a root; a non-followed stranger's reply does NOT           |
//! |      | attribute and does NOT surface as its own row. A feed that ignored the   |
//! |      | follow set (treated every author as followed — the "global feed"         |
//! |      | regression) would attribute the stranger, so this assertion FAILS on it  |
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

/// Proves the follow set is load-bearing for the engine's root-indexed
/// attribution — the one "is this a *following* timeline?" axis this synthetic
/// harness can observe.
///
/// A reply from a FOLLOWED author (here the signed-in account itself, which is
/// self-included in its own follow set) attributes to the root; a reply from a
/// NON-followed stranger does NOT attribute at all, and does NOT surface as its
/// own row. These are the decisive, FALSIFIABLE assertions: if the feed ignored
/// the follow set and treated every author as followed (the "global feed"
/// regression), the stranger's reply WOULD attribute and this gate would FAIL.
///
/// ## Why NOT a "standalone non-followed root is absent" assertion
///
/// Root-AUTHOR following (a standalone note from someone you don't follow never
/// appearing) is NOT enforced in the feed engine — by design. `RootIndexedFeed`
/// admits every root it observes; the follow gate for root admission is the
/// kernel's ingest-layer relevance filter (the active account's
/// `timeline_authors`), and a non-followed root only reaches the engine at all
/// via explicit event-ref hydration when a FOLLOWED reply references it
/// (see `docs/perf/op-centric-feed-architecture.md` §B and the engine's
/// `crates/nmp-feed/src/root_indexed/engine/ingest.rs::ingest_root`).
///
/// This harness seeds events through the synthetic-injection seam
/// (`nmp_app_inject_signed_event_json` → `IngestPreVerifiedEvents`), which fans
/// every injected event out to observers UNCONDITIONALLY — it deliberately
/// bypasses the kernel `timeline_authors` relevance gate (that is its whole
/// purpose: a stand-in for "events arriving from relays"). So a standalone
/// non-followed root injected here DOES render — exactly as the intentional
/// `standalone_note_renders_as_root_card` test in `nmp-app-chirp` asserts. An
/// "absent" assertion would therefore be UNSATISFIABLE through this seam and
/// would be testing the kernel acquisition/relevance layer, not the OP-feed
/// following contract this gate exists to prove. Root-relevance following is a
/// kernel-ingest / subscription concern, proved by its own kernel tests.
#[test]
fn g3_follow_set_is_load_bearing_for_attribution() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let demo = DemoApp::login(DEMO_NSEC);
    let viewer_hex = demo.viewer().to_string();

    let root_author = Keys::generate(); // the root note's author
    let stranger = Keys::generate(); // a NON-followed replier

    // A root note surfaces as a card.
    let root = note(&root_author, 2_000, "root of a thread");
    let root_id = root.id.to_hex();
    assert!(demo.ingest(&root), "root must verify");

    // The signed-in account (self-included follow) replies to the root.
    let viewer_keys = Keys::parse(DEMO_NSEC).expect("valid demo nsec");
    let followed_reply = reply(&viewer_keys, 2_100, &root_id, "I follow this thread");
    assert!(demo.ingest(&followed_reply), "followed reply must verify");

    // A non-followed stranger replies to the same root.
    let stranger_reply = reply(&stranger, 2_200, &root_id, "I am not followed");
    assert!(demo.ingest(&stranger_reply), "stranger reply must verify");

    // Wait until the root card carries the followed-author attribution.
    let rows = demo.rows_when(Duration::from_secs(5), |rows| {
        rows.iter().any(|r| {
            r.author_pubkey == root_author.public_key().to_hex()
                && !r.attribution_pubkeys.is_empty()
        })
    });

    let root_row = rows
        .iter()
        .find(|r| r.author_pubkey == root_author.public_key().to_hex())
        .expect("G3 DX GAP: the root note must render as a card");

    assert!(
        root_row.attribution_pubkeys.contains(&viewer_hex),
        "G3 DX GAP: a followed author's reply must attribute to the root \
         (attribution={:?}, expected to contain viewer {viewer_hex})",
        root_row.attribution_pubkeys
    );
    // The decisive, FALSIFIABLE proof that the follow set is load-bearing: the
    // NON-followed stranger's reply must NOT attribute to the root. A feed that
    // ignored the follow set (treated every author as followed — i.e. a global
    // feed) would run the same attribution path for the stranger and this would
    // contain the stranger's pubkey, FAILING the gate. (Verified by reasoning:
    // attribution is gated in `RootIndexedFeed::ingest` on `follow(author)`;
    // forcing that predicate to `true` for all authors makes the stranger
    // attribute and trips this assertion.)
    assert!(
        !root_row
            .attribution_pubkeys
            .contains(&stranger.public_key().to_hex()),
        "G3 DX GAP: a NON-followed reply must NOT attribute — this would mean the \
         feed ignores the follow set (a GLOBAL feed), not a following timeline \
         (attribution={:?})",
        root_row.attribution_pubkeys
    );

    // Reinforcing assertion on the SAME axis: a non-followed reply is dropped by
    // the engine, so it never surfaces as its own row either. Neither the
    // stranger's pubkey nor the stranger's reply body may appear as a rendered
    // card. (Belt-and-suspenders against a regression that rendered dropped
    // replies as standalone rows.)
    assert!(
        !rows
            .iter()
            .any(|r| r.author_pubkey == stranger.public_key().to_hex()
                || r.content.contains("I am not followed")),
        "G3 DX GAP: a NON-followed reply surfaced as its own timeline row — the \
         follow set is not gating the engine. Rendered rows: {:?}",
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
        "FeedShape::RootIndexed",
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
