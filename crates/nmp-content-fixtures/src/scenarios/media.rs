//! Media-block scenarios (S-MD01 … S-MD03).
//!
//! These exercise the grouper's URL-extension classifier
//! (`crates/nmp-content/src/grouper.rs::media_kind_for_url`) — the pure rule
//! that lifts an `https://…/foo.<ext>` URL out of the inline run and into a
//! `Segment::Media { kind: MediaKind::{Image,Video,Audio} }` block. No MIME
//! sniff, no HTTP — extension only, per `content-rendering.md` §10.
//!
//! These scenarios deliberately overlap with the S-T04/T05 / S-T06 cases in
//! `text.rs` but isolate ONE media kind per scenario id so the wire-fixture
//! consumers (iOS / Android) can pin each kind's rendered shape
//! independently.

use crate::dto::ScenarioDto;
use crate::embed_store::EmbedStore;
use crate::identities::Identities;

use super::scenario;

const BASE: u64 = 1_700_060_000;

/// Build every media-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let a = &ids.alice;
    let store = EmbedStore::default();
    let mut out = Vec::new();

    // S-MD01: single image URL -> Segment::Media { kind: Image }.
    let e = a.sign(
        1,
        BASE,
        vec![],
        "field notes from the install \
         https://nmp.test/img/install.jpg",
    );
    out.push(scenario(
        "S-MD01",
        "media",
        "Single image URL -> media block",
        "grouper post-pass -> Segment::Media { kind: Image }",
        &e,
        vec![],
        &store,
    ));

    // S-MD02: single video URL -> Segment::Media { kind: Video }.
    let e = a.sign(
        1,
        BASE + 1,
        vec![],
        "demo reel https://nmp.test/v/demo.mp4",
    );
    out.push(scenario(
        "S-MD02",
        "media",
        "Single video URL -> media block",
        "grouper post-pass -> Segment::Media { kind: Video }",
        &e,
        vec![],
        &store,
    ));

    // S-MD03: single audio URL -> Segment::Media { kind: Audio }.
    let e = a.sign(
        1,
        BASE + 2,
        vec![],
        "voice memo https://nmp.test/a/memo.mp3",
    );
    out.push(scenario(
        "S-MD03",
        "media",
        "Single audio URL -> media block",
        "grouper post-pass -> Segment::Media { kind: Audio }",
        &e,
        vec![],
        &store,
    ));

    out
}
