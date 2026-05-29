//! Gallery application state and live-kernel layout.

use std::cell::RefCell;
use std::io::Read as _;
use std::sync::{Arc, Mutex};

use iced::widget::image::Handle as ImageHandle;
use iced::widget::{button, column, container, row, rule, scrollable, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Subscription};
use serde_json::Value;
use tokio::sync::mpsc;

use nmp_content::embed_projection::EmbedKindProjection;
use nmp_gallery_tui::content_tree_wire::{WireNode, WireUri};
use nmp_gallery_tui::data::{GalleryData, LiveProfileMap};
use nmp_gallery_tui::embed_host::EmbedHostState;
use nmp_gallery_tui::gallery::{component_at, ComponentSpec, REGISTRY_SECTIONS};
use nmp_gallery_tui::live::primary_pubkey;

use crate::bridge::GalleryBridge;
use crate::components::embed_article::ArticleCard;
use crate::components::user_avatar::UserAvatar;
use crate::components::user_card::UserCard;
use crate::components::user_name::UserName;
use crate::components::user_nip05::Nip05Badge;
use crate::components::user_npub::NpubChip;

const CONSUMER_ID: &str = "nmp-gallery-desktop.preview";

// ── Palette ──────────────────────────────────────────────────────────────────

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

// ── State ─────────────────────────────────────────────────────────────────────

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
    // Snapshot receiver for the iced subscription. Taken from bridge in new()
    // and held here so subscription() can initialize the subscription stream.
    snapshot_rx: RefCell<Option<mpsc::UnboundedReceiver<Value>>>,
}

impl GalleryApp {
    #[must_use]
    pub fn new() -> Self {
        let mut bridge = GalleryBridge::start();
        let snapshot_rx = bridge.take_snapshot_receiver();
        Self {
            bridge,
            data: GalleryData::live_initial(primary_pubkey()),
            profiles: LiveProfileMap::new(),
            embed_host: EmbedHostState::new(),
            selected: 0,
            last_rev: 0,
            avatar_url_fetching: None,
            avatar_pending: Arc::new(Mutex::new(None)),
            avatar_handle: None,
            snapshot_rx: RefCell::new(snapshot_rx),
        }
    }
}

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    Snapshot(Value),
    Select(usize),
}

// ── Subscription ──────────────────────────────────────────────────────────────

/// Custom subscription recipe: drives iced redraws from the kernel's push
/// channel without any timer poll (D8 — no polling).
struct SnapshotRecipe(std::sync::Mutex<Option<mpsc::UnboundedReceiver<Value>>>);

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
        let rx = self
            .0
            .lock()
            .expect("snapshot mutex uncontested")
            .take()
            .expect("receiver present exactly once");
        Box::pin(iced::futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|v| (Message::Snapshot(v), rx))
        }))
    }
}

pub fn subscription(app: &GalleryApp) -> Subscription<Message> {
    if let Some(rx) = app.snapshot_rx.borrow_mut().take() {
        iced::advanced::subscription::from_recipe(SnapshotRecipe(
            std::sync::Mutex::new(Some(rx)),
        ))
    } else {
        Subscription::none()
    }
}

// ── Update ────────────────────────────────────────────────────────────────────

pub fn update(app: &mut GalleryApp, message: Message) {
    match message {
        Message::Snapshot(snap) => {
            // 1. Update profiles and embed host from the kernel snapshot.
            app.profiles.update_from_snapshot(&snap);
            let embed_authors = app.embed_host.update_from_snapshot(&snap);

            // 2. Claim profiles for any newly discovered embed authors.
            for pk in &embed_authors {
                app.bridge.claim_profile(pk, CONSUMER_ID);
            }

            // 3. Claim the primary pubkey so the kind:0 fetch proceeds.
            app.bridge.claim_profile(primary_pubkey(), CONSUMER_ID);

            // 4. Claim embed event refs from the four showcase content trees.
            claim_tree_refs(&app.bridge, &app.data.embed_article.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.embed_profile.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.embed_note.tree.nodes);
            claim_tree_refs(&app.bridge, &app.data.embed_highlight.tree.nodes);

            app.last_rev += 1;

            // 5. Check if a background avatar fetch completed. Create the Handle
            //    exactly once here — never in view() — so the same Handle ID is
            //    passed to iced every frame and the GPU texture is not re-uploaded.
            if let Some(bytes) = app.avatar_pending.lock().ok().and_then(|mut s| s.take()) {
                app.avatar_handle = Some(ImageHandle::from_bytes(bytes));
            }

            // 6. Start fetching the primary pubkey's picture_url if it changed.
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
            bridge.claim_event(&u.uri, CONSUMER_ID);
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

    for section in REGISTRY_SECTIONS {
        col = col.push(
            text(section.label)
                .size(12)
                .font(Font::MONOSPACE)
                .style(|_| text::Style {
                    color: Some(SECTION_BLUE),
                }),
        );

        for comp in section.components {
            let idx = flat_index;
            let is_active = idx == selected;
            flat_index += 1;

            let label = comp.label;
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

    let content = render_component(spec, app);

    let body = column![heading, content]
        .spacing(16)
        .padding(16)
        .width(Length::Fill);

    container(scrollable(body))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ── Component rendering ───────────────────────────────────────────────────────

fn render_component<'a>(spec: ComponentSpec, app: &'a GalleryApp) -> Element<'a, Message> {
    let primary = app.profiles.resolve(primary_pubkey());

    match spec.id {
        "user-avatar" => {
            let mut av = UserAvatar::new(&primary.pubkey)
                .display_name(primary.display_name.as_deref())
                .size(96.0);
            if let Some(handle) = app.avatar_handle.clone() {
                av = av.picture_handle(handle);
            }
            let avatar = av.into_element::<Message>();

            column![
                container(avatar)
                    .align_x(Alignment::Center)
                    .width(Length::Fill),
                container(
                    text(format!("Pubkey: {}", primary.npub_short))
                        .size(12)
                        .style(|_| text::Style {
                            color: Some(MUTED_TEXT)
                        })
                )
                .align_x(Alignment::Center)
                .width(Length::Fill),
            ]
            .spacing(8)
            .into()
        }

        "user-name" => UserName::from_profile(&primary).into_element::<Message>(),

        "user-nip05" => match Nip05Badge::from_profile(&primary) {
            Some(b) => b.into_element::<Message>(),
            None => text("no nip05 yet")
                .size(13)
                .style(|_| text::Style {
                    color: Some(MUTED_TEXT),
                })
                .into(),
        },

        "user-npub" => NpubChip::from_profile(&primary).into_element::<Message>(),

        "user-card" => {
            let mut card = UserCard::from_profile(&primary);
            if let Some(handle) = app.avatar_handle.clone() {
                card = card.avatar_handle(handle);
            }
            card.into_element::<Message>()
        }

        "content-core" => {
            let ex = &app.data.content_core;
            content_tree_info(&ex.scenario_id, &ex.title, &ex.tree.nodes)
        }
        "content-view" => {
            let ex = &app.data.content_view;
            content_tree_info(&ex.scenario_id, &ex.title, &ex.tree.nodes)
        }
        "content-mention-chip" => {
            let ex = &app.data.content_mention_chip;
            content_tree_info(&ex.scenario_id, &ex.title, &ex.tree.nodes)
        }
        "content-minimal" => {
            let ex = &app.data.content_minimal;
            content_tree_info(&ex.scenario_id, &ex.title, &ex.tree.nodes)
        }
        "content-media-grid" => {
            let ex = &app.data.content_media_grid;
            content_tree_info(&ex.scenario_id, &ex.title, &ex.tree.nodes)
        }
        "content-quote-card" => {
            let ex = &app.data.content_quote_card;
            content_tree_info(&ex.scenario_id, &ex.title, &ex.tree.nodes)
        }

        "embed-article" => render_embed(
            &app.data.embed_article.tree.nodes,
            &app.embed_host,
            |proj| {
                if let EmbedKindProjection::Article(a) = proj {
                    Some(ArticleCard::new(a).into_element())
                } else {
                    None
                }
            },
        ),
        "embed-profile" => render_embed(
            &app.data.embed_profile.tree.nodes,
            &app.embed_host,
            |proj| {
                if let EmbedKindProjection::Profile(p) = proj {
                    Some(
                        text(format!(
                            "Profile: {}",
                            p.display_name.as_deref().unwrap_or(&p.pubkey[..8])
                        ))
                        .size(14)
                        .into(),
                    )
                } else {
                    None
                }
            },
        ),
        "embed-note" => render_embed(&app.data.embed_note.tree.nodes, &app.embed_host, |proj| {
            if let EmbedKindProjection::ShortNote(n) = proj {
                Some(
                    column![
                        text(n.author_display_name.as_deref().unwrap_or("Unknown"))
                            .size(13)
                            .font(iced::Font {
                                weight: iced::font::Weight::Bold,
                                ..iced::Font::default()
                            }),
                        text(format!("kind:1 · {}", &n.author_pubkey[..12]))
                            .size(12)
                            .style(|_| text::Style {
                                color: Some(INACTIVE_TEXT)
                            }),
                    ]
                    .spacing(4)
                    .into(),
                )
            } else {
                None
            }
        }),
        "embed-highlight" => render_embed(
            &app.data.embed_highlight.tree.nodes,
            &app.embed_host,
            |proj| {
                if let EmbedKindProjection::Highlight(h) = proj {
                    Some(
                        text(format!("\u{201c}{}\u{201d}", h.highlighted_text))
                            .size(13)
                            .style(|_| text::Style {
                                color: Some(INACTIVE_TEXT),
                            })
                            .into(),
                    )
                } else {
                    None
                }
            },
        ),

        _ => text("Unknown component").into(),
    }
}

/// Render an embed showcase: find the first EventRef in `nodes`, look it up
/// in the embed host, call `render` on the resolved projection. Shows a
/// "fetching…" placeholder until the event arrives.
fn render_embed<'a, F>(
    nodes: &'a [WireNode],
    host: &'a EmbedHostState,
    render: F,
) -> Element<'a, Message>
where
    F: Fn(&'a EmbedKindProjection) -> Option<Element<'a, Message>>,
{
    let envelope = nodes.iter().find_map(|n| {
        let uri = match n {
            WireNode::EventRef(u) => Some(u),
            WireNode::Mention(u) => Some(u),
            _ => None,
        }?;
        host.current_envelopes().get(&uri.primary_id)
    });

    if let Some(env) = envelope {
        if let Some(el) = render(&env.projection) {
            return el;
        }
        text("Unexpected projection kind").size(13).into()
    } else {
        text("Fetching from relay…")
            .size(13)
            .style(|_| text::Style {
                color: Some(MUTED_TEXT),
            })
            .into()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn content_tree_info<'a>(
    scenario_id: &str,
    title: &str,
    nodes: &[WireNode],
) -> Element<'a, Message> {
    let snippet: String = nodes
        .iter()
        .filter_map(|n| {
            if let WireNode::Text(t) = n {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect();

    column![
        text(format!("scenario: {scenario_id}"))
            .size(12)
            .font(Font::MONOSPACE)
            .style(|_| text::Style {
                color: Some(MUTED_TEXT)
            }),
        text(format!("title: {title}")).size(13),
        text(format!("nodes: {}", nodes.len())).size(13),
        rule::horizontal(1),
        text(if snippet.is_empty() {
            "(no plain-text nodes)".to_string()
        } else {
            snippet
        })
        .size(13)
        .style(|_| text::Style {
            color: Some(INACTIVE_TEXT)
        }),
    ]
    .spacing(6)
    .into()
}
