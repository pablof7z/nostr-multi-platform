//! Gallery application state and live-kernel layout.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::sync::{Arc, Mutex};

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{button, column, container, row, rule, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Font, Length, Subscription};
use tokio::sync::mpsc;

use nmp_gallery_tui::content_tree_wire::{WireNode, WireUri};
use nmp_gallery_tui::data::{GalleryData, LiveProfileMap};
use nmp_gallery_tui::embed_host::EmbedHostState;
use nmp_gallery_tui::gallery::{component_at, component_index, registry_sections};
use nmp_gallery_tui::live::{primary_pubkey, GalleryTypedSnapshot};

use crate::bridge::GalleryBridge;

mod component_render;

const CONSUMER_ID: &str = "nmp-gallery-desktop.preview";

const SECTION_BLUE: Color = Color {
    r: 0.490,
    g: 0.827,
    b: 0.988,
    a: 1.0,
};
const INACTIVE_TEXT: Color = Color {
    r: 0.796,
    g: 0.835,
    b: 0.894,
    a: 1.0,
};
const MUTED_TEXT: Color = Color {
    r: 0.580,
    g: 0.639,
    b: 0.722,
    a: 1.0,
};
const ACTIVE_BG: Color = Color {
    r: 0.118,
    g: 0.161,
    b: 0.231,
    a: 1.0,
};
const DARK_BG: Color = Color {
    r: 0.059,
    g: 0.082,
    b: 0.118,
    a: 1.0,
};

pub struct GalleryApp {
    bridge: GalleryBridge,
    data: GalleryData,
    profiles: LiveProfileMap,
    embed_host: EmbedHostState,
    selected: usize,
    last_rev: u64,
    // Avatar image: URL being fetched, pending bytes slot, and the cached
    // Handle created once on arrival. Storing the Handle (not raw bytes) is
    // critical — Handle has a stable ID so iced reuses the same GPU texture
    // every frame instead of re-uploading on each render call.
    avatar_url_fetching: Option<String>,
    avatar_pending: Arc<Mutex<Option<Vec<u8>>>>,
    avatar_handle: Option<ImageHandle>,
    media_url_fetching: BTreeSet<String>,
    media_pending: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    media_handles: BTreeMap<String, ImageHandle>,
    // Snapshot receiver for the iced subscription. Taken from bridge in new()
    // and held in a shared slot. `subscription()` is called by iced after every
    // update and must return a *stable* recipe (same hash, every call) or iced
    // diffs the subscription set, sees the recipe vanish, and tears the stream
    // down — which froze the UI after the first ~7 snapshots. The recipe shares
    // this Arc and takes the receiver exactly once inside `stream()`.
    snapshot_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<GalleryTypedSnapshot>>>>,
}

impl GalleryApp {
    #[must_use]
    pub fn new() -> Self {
        let mut bridge = GalleryBridge::start();
        let snapshot_rx = bridge.take_snapshot_receiver();
        // Deterministic initial selection for screenshot capture: if
        // NMP_GALLERY_COMPONENT names a known component slug, start on it;
        // otherwise fall back to the first component (index 0). This mirrors
        // the iOS gallery's "direct component" entry pattern.
        let selected = std::env::var("NMP_GALLERY_COMPONENT")
            .ok()
            .map(|slug| component_index(&slug))
            .unwrap_or(0);
        Self {
            bridge,
            data: GalleryData::live_initial(primary_pubkey()),
            profiles: LiveProfileMap::new(),
            embed_host: EmbedHostState::new(),
            selected,
            last_rev: 0,
            avatar_url_fetching: None,
            avatar_pending: Arc::new(Mutex::new(None)),
            avatar_handle: None,
            media_url_fetching: BTreeSet::new(),
            media_pending: Arc::new(Mutex::new(Vec::new())),
            media_handles: BTreeMap::new(),
            snapshot_rx: Arc::new(Mutex::new(snapshot_rx)),
        }
    }
}

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    Snapshot(Arc<GalleryTypedSnapshot>),
    Select(usize),
}

// ── Subscription ──────────────────────────────────────────────────────────────

/// Custom subscription recipe: drives iced redraws from the kernel's push
/// channel without any timer poll (D8 — no polling).
struct SnapshotRecipe(Arc<Mutex<Option<mpsc::UnboundedReceiver<GalleryTypedSnapshot>>>>);

impl iced::advanced::subscription::Recipe for SnapshotRecipe {
    type Output = Message;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        use std::hash::Hash;
        "gallery-snapshot".hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: iced::advanced::subscription::EventStream,
    ) -> iced::futures::stream::BoxStream<'static, Message> {
        // iced builds the stream exactly once per subscription identity (the
        // stable hash above). Take the receiver from the shared slot here, so
        // the running stream owns it for its lifetime. Subsequent recipe
        // constructions (from re-evaluating `subscription()` after each update)
        // share the same Arc but never reach `stream()` again.
        let rx = self
            .0
            .lock()
            .expect("snapshot mutex uncontested")
            .take()
            .expect("receiver present exactly once for the stream's lifetime");
        Box::pin(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv()
                .await
                .map(|v| (Message::Snapshot(Arc::new(v)), rx))
        }))
    }
}

pub fn subscription(app: &GalleryApp) -> Subscription<Message> {
    // Always return the same recipe (same hash). iced re-evaluates this after
    // every update and diffs the returned set against the running one by hash;
    // returning `Subscription::none()` on later calls would make iced believe
    // the subscription was removed and tear down the snapshot stream. The
    // recipe shares `snapshot_rx` and takes the receiver once inside `stream()`.
    iced::advanced::subscription::from_recipe(SnapshotRecipe(Arc::clone(&app.snapshot_rx)))
}

// ── Update ────────────────────────────────────────────────────────────────────

pub fn update(app: &mut GalleryApp, message: Message) {
    match message {
        Message::Snapshot(snap) => {
            // 1. Update profiles and embed host from the typed kernel snapshot.
            //    The embed host rebuilds its envelope map; we deliberately
            //    ignore its `authors_needing_profile` return value now.
            //    Embed-author kind:0 claiming is component-owned (iOS #833):
            //    each embed renderer that shows an author byline claims that
            //    author itself via `claim_and_resolve_author` at render time.
            //    The central pre-warm loop that claimed EVERY embed author on
            //    EVERY snapshot tick (regardless of whether it was being
            //    displayed) is gone — that was a component-owned-reactivity
            //    violation. NO event triggers a reactive kernel kind:0 fetch;
            //    fetching kind:0 is the presentation layer's concern, owned by
            //    the component that displays the author.
            app.profiles.update_from_typed(&snap);
            let _ = app.embed_host.update_from_typed(&snap);

            // 2. Resolve the primary pubkey so the kind:0 fetch proceeds. This
            //    is the app's own identity bootstrap for the user-* showcase
            //    components — a separate path from embed author bylines.
            app.bridge.resolve_profile(primary_pubkey(), CONSUMER_ID);

            // 3. Claim embed event refs from the four showcase content trees.
            claim_tree_refs(&app.bridge, &app.data.embed_article.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.embed_profile.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.embed_note.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.embed_highlight.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.content_core.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.content_view.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.content_mention_chip.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.content_minimal.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.content_media_grid.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.content_quote_card.tree.nodes);

            app.last_rev += 1;

            // 4. Check if a background avatar fetch completed. Create the Handle
            //    exactly once here — never in view() — so the same Handle ID is
            //    passed to iced every frame and the GPU texture is not re-uploaded.
            if let Some(bytes) = app.avatar_pending.lock().ok().and_then(|mut s| s.take()) {
                app.avatar_handle = Some(ImageHandle::from_bytes(bytes));
            }
            if let Ok(mut pending) = app.media_pending.lock() {
                for (url, bytes) in pending.drain(..) {
                    app.media_handles
                        .insert(url, ImageHandle::from_bytes(bytes));
                }
            }

            // 5. Start fetching the primary pubkey's picture_url if it changed.
            let primary = app.profiles.resolve(primary_pubkey());
            if let Some(url) = primary.picture_url {
                if app.avatar_url_fetching.as_deref() != Some(&url) {
                    app.avatar_url_fetching = Some(url.clone());
                    let pending = Arc::clone(&app.avatar_pending);
                    std::thread::spawn(move || {
                        if let Some(bytes) = fetch_image_sync(&url) {
                            if let Ok(mut slot) = pending.lock() {
                                *slot = Some(bytes);
                            }
                        }
                    });
                }
            }

            // Fetch only presentation images referenced by the resolved content
            // projections. Event/content identity and media URL discovery stay
            // Rust-owned; iced stores stable handles for drawing.
            for url in content_media_urls(app) {
                if app.media_handles.contains_key(&url) || !app.media_url_fetching.insert(url.clone()) {
                    continue;
                }
                let pending = Arc::clone(&app.media_pending);
                std::thread::spawn(move || {
                    if let Some(bytes) = fetch_image_sync(&url) {
                        if let Ok(mut slot) = pending.lock() {
                            slot.push((url, bytes));
                        }
                    }
                });
            }
        }
        Message::Select(i) => {
            app.selected = i;
        }
    }
}

/// Claim all EventRef + Mention URIs in a content tree. Idempotent — the
/// kernel deduplicates per (uri, consumer_id); re-claiming every tick is
/// deliberate so claims stick once a relay connects (W1 open-Q #3).
fn claim_tree_refs(bridge: &GalleryBridge, nodes: &[WireNode]) {
    for node in nodes {
        let uri: Option<&WireUri> = match node {
            WireNode::EventRef(u) => Some(u),
            WireNode::Mention(u) => Some(u),
            _ => None,
        };
        if let Some(u) = uri {
            bridge.resolve_event_uri(&u.uri, CONSUMER_ID);
        }
    }
}

/// Synchronous image fetch via ureq. Runs inside a background thread so it
/// never blocks the iced event loop.
fn fetch_image_sync(url: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    ureq::get(url)
        .call()
        .ok()?
        .into_reader()
        .take(8 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

fn content_media_urls(app: &GalleryApp) -> Vec<String> {
    let envelopes = app.embed_host.current_envelopes();
    let mut urls = crate::components::content_view::referenced_media_urls(
        &app.data.content_media_grid.tree,
        envelopes,
    );
    urls.extend(crate::components::content_view::referenced_media_urls(
        &app.data.content_view.tree,
        envelopes,
    ));
    urls.extend(crate::components::content_view::referenced_media_urls(
        &app.data.content_quote_card.tree,
        envelopes,
    ));
    urls.sort();
    urls.dedup();
    urls
}

// ── View ──────────────────────────────────────────────────────────────────────

pub fn view(app: &GalleryApp) -> Element<'_, Message> {
    let header = container(
        text(format!("NMP Desktop Gallery | rev {}", app.last_rev))
            .size(16)
            .font(Font::MONOSPACE),
    )
    .width(Length::Fill)
    .padding([8, 16])
    .style(|_| container::Style {
        background: Some(Background::Color(DARK_BG)),
        ..Default::default()
    });

    let sidebar = build_sidebar(app.selected);
    let detail = build_detail(app);

    let body = row![sidebar, rule::vertical(1), detail].height(Length::Fill);

    column![header, body].height(Length::Fill).into()
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn build_sidebar(selected: usize) -> Element<'static, Message> {
    let mut flat_index: usize = 0;
    let mut col = column![
        text("Components")
            .size(13)
            .font(Font::MONOSPACE)
            .style(|_| text::Style {
                color: Some(MUTED_TEXT)
            }),
        Space::new().height(Length::Fixed(6.0)),
    ]
    .spacing(2)
    .padding([8, 8]);

    for section in registry_sections() {
        col = col.push(
            text(section.label.as_str())
                .size(12)
                .font(Font::MONOSPACE)
                .style(|_| text::Style {
                    color: Some(SECTION_BLUE),
                }),
        );

        for comp in &section.components {
            let idx = flat_index;
            let is_active = idx == selected;
            flat_index += 1;

            let label = comp.label.as_str();
            let btn = button(text(label).size(13).style(move |_| text::Style {
                color: Some(if is_active {
                    Color::WHITE
                } else {
                    INACTIVE_TEXT
                }),
            }))
            .on_press(Message::Select(idx))
            .width(Length::Fill)
            .padding([4, 8])
            .style(move |_, _| button::Style {
                background: if is_active {
                    Some(Background::Color(ACTIVE_BG))
                } else {
                    None
                },
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                text_color: if is_active {
                    Color::WHITE
                } else {
                    INACTIVE_TEXT
                },
                ..Default::default()
            });

            col = col.push(btn);
        }

        col = col.push(Space::new().height(Length::Fixed(4.0)));
    }

    container(scrollable(col))
        .width(Length::Fixed(220.0))
        .height(Length::Fill)
        .into()
}

// ── Detail panel ──────────────────────────────────────────────────────────────

fn build_detail(app: &GalleryApp) -> Element<'_, Message> {
    let spec = component_at(app.selected);

    let heading = column![
        text(spec.label).size(20),
        text(spec.description).size(13).style(|_| text::Style {
            color: Some(MUTED_TEXT)
        }),
        rule::horizontal(1),
    ]
    .spacing(4);

    let content = component_render::render_component(spec, app);

    let body = column![heading, content]
        .spacing(16)
        .padding(16)
        .width(Length::Fill);

    container(scrollable(body))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
