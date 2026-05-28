//! Gallery application state and live-kernel layout.

use std::time::Duration;

use iced::widget::{button, column, container, row, rule, scrollable, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Subscription};

use nmp_gallery_tui::content_tree_wire::WireNode;
use nmp_gallery_tui::data::{GalleryData, LiveProfileMap};
use nmp_gallery_tui::gallery::{component_at, ComponentSpec, REGISTRY_SECTIONS};
use nmp_gallery_tui::live::PRIMARY_PUBKEY;

use crate::bridge::GalleryBridge;
use crate::components::user_avatar::UserAvatar;
use crate::components::user_card::UserCard;
use crate::components::user_name::UserName;
use crate::components::user_nip05::Nip05Badge;
use crate::components::user_npub::NpubChip;

// ── Palette ──────────────────────────────────────────────────────────────────

const SECTION_BLUE: Color = Color { r: 0.490, g: 0.827, b: 0.988, a: 1.0 };
const INACTIVE_TEXT: Color = Color { r: 0.796, g: 0.835, b: 0.894, a: 1.0 };
const MUTED_TEXT: Color = Color { r: 0.580, g: 0.639, b: 0.722, a: 1.0 };
const ACTIVE_BG: Color = Color { r: 0.118, g: 0.161, b: 0.231, a: 1.0 };
const DARK_BG: Color = Color { r: 0.059, g: 0.082, b: 0.118, a: 1.0 };

// ── State ─────────────────────────────────────────────────────────────────────

pub struct GalleryApp {
    bridge: GalleryBridge,
    data: GalleryData,
    profiles: LiveProfileMap,
    pub selected: usize,
    pub last_rev: u64,
}

impl GalleryApp {
    #[must_use]
    pub fn new() -> Self {
        Self {
            bridge: GalleryBridge::start(),
            data: GalleryData::live_initial(PRIMARY_PUBKEY),
            profiles: LiveProfileMap::new(),
            selected: 0,
            last_rev: 0,
        }
    }
}

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    Poll,
    Select(usize),
}

// ── Subscription ──────────────────────────────────────────────────────────────

pub fn subscription(_app: &GalleryApp) -> Subscription<Message> {
    iced::time::every(Duration::from_millis(250)).map(|_instant| Message::Poll)
}

// ── Update ────────────────────────────────────────────────────────────────────

pub fn update(app: &mut GalleryApp, message: Message) {
    match message {
        Message::Poll => {
            if let Some(snap) = app.bridge.take_snapshot() {
                app.profiles.update_from_snapshot(&snap);
                app.last_rev += 1;
            }
            app.bridge
                .claim_profile(PRIMARY_PUBKEY, "nmp-gallery-desktop.preview");
        }
        Message::Select(i) => {
            app.selected = i;
        }
    }
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
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(Background::Color(DARK_BG)),
        ..Default::default()
    });

    let sidebar = build_sidebar(app.selected);
    let detail = build_detail(app);

    let body = row![
        sidebar,
        rule::vertical(1),
        detail,
    ]
    .height(Length::Fill);

    column![header, body]
        .height(Length::Fill)
        .into()
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn build_sidebar(selected: usize) -> Element<'static, Message> {
    let mut flat_index: usize = 0;
    let mut col = column![
        text("Components")
            .size(13)
            .font(Font::MONOSPACE)
            .style(|_theme: &iced::Theme| text::Style {
                color: Some(MUTED_TEXT),
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
                .style(|_theme: &iced::Theme| text::Style {
                    color: Some(SECTION_BLUE),
                }),
        );

        for comp in section.components {
            let idx = flat_index;
            let is_active = idx == selected;
            flat_index += 1;

            let label = comp.label;
            let btn = if is_active {
                button(
                    text(label)
                        .size(13)
                        .style(|_theme: &iced::Theme| text::Style {
                            color: Some(Color::WHITE),
                        }),
                )
                .on_press(Message::Select(idx))
                .width(Length::Fill)
                .padding([4, 8])
                .style(|_theme: &iced::Theme, _status| button::Style {
                    background: Some(Background::Color(ACTIVE_BG)),
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    text_color: Color::WHITE,
                    ..Default::default()
                })
            } else {
                button(
                    text(label)
                        .size(13)
                        .style(|_theme: &iced::Theme| text::Style {
                            color: Some(INACTIVE_TEXT),
                        }),
                )
                .on_press(Message::Select(idx))
                .width(Length::Fill)
                .padding([4, 8])
                .style(|_theme: &iced::Theme, _status| button::Style {
                    background: None,
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    text_color: INACTIVE_TEXT,
                    ..Default::default()
                })
            };

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
    let primary = app.profiles.resolve(PRIMARY_PUBKEY);

    let heading = column![
        text(spec.label).size(20),
        text(spec.description)
            .size(13)
            .style(|_theme: &iced::Theme| text::Style {
                color: Some(MUTED_TEXT),
            }),
        rule::horizontal(1),
    ]
    .spacing(4);

    let content = render_component(spec, app);

    let body = column![heading, content]
        .spacing(16)
        .padding(16)
        .width(Length::Fill);

    // Wrap in scrollable so tall content doesn't overflow.
    container(scrollable(body))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ── Component rendering ───────────────────────────────────────────────────────

fn render_component<'a>(spec: ComponentSpec, app: &'a GalleryApp) -> Element<'a, Message> {
    let primary = app.profiles.resolve(PRIMARY_PUBKEY);

    match spec.id {
        "user-avatar" => {
            let avatar = UserAvatar::new(&primary.pubkey)
                .display_name(primary.display_name.as_deref())
                .size(64.0)
                .into_element::<Message>();

            let npub_label = text(format!("Pubkey: {}", primary.npub_short))
                .size(12)
                .style(|_theme: &iced::Theme| text::Style {
                    color: Some(MUTED_TEXT),
                });

            column![
                container(avatar)
                    .align_x(Alignment::Center)
                    .width(Length::Fill),
                container(npub_label)
                    .align_x(Alignment::Center)
                    .width(Length::Fill),
            ]
            .spacing(8)
            .into()
        }

        "user-name" => UserName::from_profile(&primary).into_element::<Message>(),

        "user-nip05" => match Nip05Badge::from_profile(&primary) {
            Some(badge) => badge.into_element::<Message>(),
            None => text("no nip05 yet")
                .size(13)
                .style(|_theme: &iced::Theme| text::Style {
                    color: Some(MUTED_TEXT),
                })
                .into(),
        },

        "user-npub" => NpubChip::from_profile(&primary).into_element::<Message>(),

        "user-card" => UserCard::from_profile(&primary).into_element::<Message>(),

        "content-core" => {
            let ex = &app.data.content_core;
            content_tree_info(ex.scenario_id.as_str(), ex.title.as_str(), &ex.tree.nodes)
        }

        "content-view" => {
            let ex = &app.data.content_view;
            content_tree_info(ex.scenario_id.as_str(), ex.title.as_str(), &ex.tree.nodes)
        }

        "content-mention-chip" => {
            let ex = &app.data.content_mention_chip;
            content_tree_info(ex.scenario_id.as_str(), ex.title.as_str(), &ex.tree.nodes)
        }

        "content-minimal" => {
            let ex = &app.data.content_minimal;
            content_tree_info(ex.scenario_id.as_str(), ex.title.as_str(), &ex.tree.nodes)
        }

        "content-media-grid" => {
            let ex = &app.data.content_media_grid;
            content_tree_info(ex.scenario_id.as_str(), ex.title.as_str(), &ex.tree.nodes)
        }

        "content-quote-card" => {
            let ex = &app.data.content_quote_card;
            content_tree_info(ex.scenario_id.as_str(), ex.title.as_str(), &ex.tree.nodes)
        }

        "embed-article" => embed_placeholder("embed-article", "Embedded Article"),
        "embed-profile" => embed_placeholder("embed-profile", "Embedded Profile"),
        "embed-note" => embed_placeholder("embed-note", "Embedded Note"),
        "embed-highlight" => embed_placeholder("embed-highlight", "Embedded Highlight"),

        _ => text("Unknown component").into(),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn content_tree_info<'a>(
    scenario_id: &str,
    title: &str,
    nodes: &[WireNode],
) -> Element<'a, Message> {
    let node_count = nodes.len();
    let snippet: String = nodes
        .iter()
        .filter_map(|n| match n {
            WireNode::Text(t) => Some(t.as_str()),
            _ => None,
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
            .style(|_theme: &iced::Theme| text::Style {
                color: Some(MUTED_TEXT),
            }),
        text(format!("title: {title}")).size(13),
        text(format!("nodes: {node_count}")).size(13),
        rule::horizontal(1),
        text(if snippet.is_empty() {
            "(no plain-text nodes)".to_string()
        } else {
            snippet
        })
        .size(13)
        .style(|_theme: &iced::Theme| text::Style {
            color: Some(INACTIVE_TEXT),
        }),
    ]
    .spacing(6)
    .into()
}

fn embed_placeholder<'a>(id: &str, label: &str) -> Element<'a, Message> {
    column![
        text(format!("Embed showcase — {label}")).size(14),
        text("Claims wired via EventClaimSink; resolve via live kernel when claimed.")
            .size(12)
            .style(|_theme: &iced::Theme| text::Style {
                color: Some(MUTED_TEXT),
            }),
        text(format!("id: {id}"))
            .size(11)
            .font(Font::MONOSPACE)
            .style(|_theme: &iced::Theme| text::Style {
                color: Some(MUTED_TEXT),
            }),
    ]
    .spacing(8)
    .into()
}
