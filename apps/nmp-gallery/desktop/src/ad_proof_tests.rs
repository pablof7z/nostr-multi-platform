//! #2927 — desktop proof that a NIP-AD resolved-collection row renders through
//! the shell's EXISTING per-kind embed renderer.
//!
//! Deterministic (no network): it drives a kind:30023 [`AdCollectionRow`] — the
//! exact shape `open_ad_collection` delivers for `https://trellis.rs/legible` —
//! through the SAME path the running app uses at view time
//! (`AdCollectionRow::to_kernel_event` → `nmp_content::resolve_embed_projection`
//! → [`ArticleCard`]) and asserts an article card is produced. The live
//! resolve half (trellis `.well-known` → relays → real event) is proven by
//! `nmp-nip-ad`'s `tests/live_trellis.rs` (NMP_AD_LIVE=1) and by the gated live
//! runtime test in the tui crate.

use nmp_content::embed_projection::EmbedKindProjection;
use nmp_content::{resolve_embed_projection, RenderContext};
use nmp_nip_ad::AdCollectionRow;

use crate::components::embed_article::ArticleCard;

/// The real `trellis.rs/legible` target coordinates (from the live
/// `.well-known/nostr.json?ad=/legible` response).
const TRELLIS_AUTHOR: &str = "3f68dede81549cc0844fafe528f1574b51e095e7491f468bd9689f87779bb81d";
const TRELLIS_D: &str = "the-machine-that-could-tell-you-why";

/// One deduped collection row exactly as `open_ad_collection` delivers it for a
/// kind:30023 hit (raw protocol values only). Stands in for the on-wire event
/// so the RENDER path is exercised deterministically.
fn trellis_article_row() -> AdCollectionRow {
    AdCollectionRow {
        id: "a".repeat(64),
        author: TRELLIS_AUTHOR.to_string(),
        kind: 30023,
        created_at: 1_720_000_000,
        content: "Some think a machine could finally explain the *why* behind every choice…"
            .to_string(),
        tags: vec![
            vec!["d".to_string(), TRELLIS_D.to_string()],
            vec![
                "title".to_string(),
                "The Machine That Could Tell You Why".to_string(),
            ],
            vec![
                "summary".to_string(),
                "On the seduction of total explanation.".to_string(),
            ],
        ],
        relay_provenance: vec!["wss://relay.primal.net".to_string()],
    }
}

#[test]
fn ad_collection_row_renders_as_article_card() {
    // The nip23 kind:30023 adapter must be registered for the resolver to yield
    // an Article (the running gallery registers it via the composition root).
    nmp_nip23::register_content_embed_projection_adapter();

    let row = trellis_article_row();

    // Exactly the app's view-time bridge: row -> KernelEvent -> per-kind
    // projection.
    let event = row.to_kernel_event();
    let projection = resolve_embed_projection(&event, &RenderContext::new());

    let EmbedKindProjection::Article(article) = &projection else {
        panic!("kind:30023 AD row must resolve to an Article projection, got {projection:?}");
    };
    assert_eq!(
        article.title.as_deref(),
        Some("The Machine That Could Tell You Why"),
        "the resolved article carries the site's title tag",
    );

    // The RESOLVED render: build the shell's existing article card from the
    // projection (the same call the `embed-article` showcase makes). Producing
    // an Element without panicking is the render-path proof.
    let _card = ArticleCard::new(article, "pablof7z").into_element::<()>();

    // Capturable running result.
    println!(
        "PROOF #2927 desktop render: AD row kind:{} d={} -> ArticleCard title={:?}",
        event.kind,
        TRELLIS_D,
        article.title.as_deref().unwrap_or("<none>"),
    );
}

/// LIVE end-to-end proof of the RUNNING app pipeline (gated on `NMP_AD_LIVE=1`
/// so CI never dials the network). Boots the real gallery runtime — which
/// injects the `Always` policy at its composition root — claims the trellis AD
/// URL exactly as `claim_tree_ad_urls` does, waits for the relay-pinned
/// collection to arrive on the typed snapshot, and renders the delivered
/// kind:30023 event through the same `resolve_embed_projection` path the view
/// uses. Proves claim -> policy -> resolve -> open_ad_collection -> ADCL
/// projection -> per-kind render, live against `https://trellis.rs/legible`.
#[test]
fn live_trellis_ad_url_resolves_and_renders_article() {
    if std::env::var("NMP_AD_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipping live NIP-AD proof: set NMP_AD_LIVE=1 to run");
        return;
    }

    use std::time::{Duration, Instant};

    use nmp_content::AdUrlState;
    use nmp_gallery_tui::live::{primary_pubkey, GalleryTypedSnapshot, LiveGallerySource, LiveKernelSink};

    const URL: &str = "https://trellis.rs/legible";
    const CONSUMER: &str = "nmp-gallery-desktop.proof";

    nmp_nip23::register_content_embed_projection_adapter();

    let mut kernel = LiveGallerySource::boot_kernel_only().expect("kernel boots");
    let sink = LiveKernelSink { app: kernel.app };
    let rx = kernel.take_receiver().expect("snapshot receiver");

    let mut profiles = nmp_core::refs::RefProfileStore::new();
    let mut events = nmp_core::refs::RefEventStore::new();

    let deadline = Instant::now() + Duration::from_secs(45);
    let mut rendered_title: Option<String> = None;

    while Instant::now() < deadline && rendered_title.is_none() {
        // Re-claim each tick so the claim sticks once relays connect (moment-1).
        sink.claim_ad_url(URL, primary_pubkey(), CONSUMER);

        let Ok(frame) = rx.recv_timeout(Duration::from_secs(2)) else {
            continue;
        };
        let snap = GalleryTypedSnapshot::from_frame_bytes(&frame, &mut profiles, &mut events);

        let AdUrlState::Resolved { projection_key } = sink.ad_url_state(URL) else {
            continue;
        };
        let Some(collection) = snap.ad_collections.get(&projection_key) else {
            continue;
        };
        // Render every row through the SAME per-kind dispatch the view uses.
        for row in &collection.rows {
            let projection =
                resolve_embed_projection(&row.to_kernel_event(), &RenderContext::new());
            if let EmbedKindProjection::Article(article) = &projection {
                // Build the real desktop widget from the live-resolved article.
                let _card = ArticleCard::new(article, "pablof7z").into_element::<()>();
                let title = article.title.clone().unwrap_or_default();
                println!(
                    "  live AD row: kind:{} id={}… author={}… title={title:?}",
                    row.kind,
                    &row.id[..8.min(row.id.len())],
                    &row.author[..8.min(row.author.len())],
                );
                rendered_title = Some(title);
            }
        }
    }

    // The proof is that the live AD URL delivered a real kind:30023 article that
    // the desktop shell rendered through its existing ArticleCard. The exact
    // title is live data (the author can republish the `d`), so we assert only
    // that a non-empty article title rendered — not a hardcoded string.
    let title = rendered_title.expect(
        "live trellis.rs/legible AD URL did not resolve to a kind:30023 article within 45s",
    );
    assert!(
        !title.trim().is_empty(),
        "resolved article should carry a title tag",
    );
    println!("PROOF #2927 desktop LIVE: {URL} -> ArticleCard title={title:?}");
}
