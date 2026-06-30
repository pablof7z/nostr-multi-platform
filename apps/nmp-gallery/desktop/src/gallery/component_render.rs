use iced::widget::{column, container, rule, text};
use iced::{Alignment, Element, Font, Length};

use nmp_content::embed_projection::EmbedKindProjection;
use nmp_gallery_tui::content_tree_wire::WireNode;
use nmp_gallery_tui::gallery::ComponentSpec;
use nmp_gallery_tui::live::primary_pubkey;

use crate::components::{
    embed_article::ArticleCard, gallery_misc, user_avatar::UserAvatar, user_card::UserCard,
    user_name::UserName, user_nip05::Nip05Badge, user_npub::NpubChip,
};

use super::{GalleryApp, Message, CONSUMER_ID, INACTIVE_TEXT, MUTED_TEXT};

pub(super) fn render_component<'a>(
    spec: ComponentSpec,
    app: &'a GalleryApp,
) -> Element<'a, Message> {
    let primary = app.profiles.resolve(primary_pubkey());

    match spec.id {
        "relay-list" => gallery_misc::relay_list(),
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
        "user-nip05" => {
            app.bridge
                .resolve_profile_card(primary_pubkey(), "nmp-gallery-desktop.user-nip05");
            match Nip05Badge::from_profile(&primary) {
                Some(b) => b.into_element::<Message>(),
                None => text("no nip05 yet")
                    .size(13)
                    .style(|_| text::Style {
                        color: Some(MUTED_TEXT),
                    })
                    .into(),
            }
        }
        "user-npub" => NpubChip::from_profile(&primary).into_element::<Message>(),
        "user-card" => {
            app.bridge
                .resolve_profile_card(primary_pubkey(), "nmp-gallery-desktop.user-card");
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
                    let author_name = claim_and_resolve_author(app, &a.author_pubkey);
                    Some(ArticleCard::new(a, author_name).into_element())
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
                let author_name = claim_and_resolve_author(app, &n.author_pubkey);
                Some(
                    column![
                        text(author_name).size(13).font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::default()
                        }),
                        text(format!(
                            "kind:1 · {}",
                            &n.author_pubkey[..12.min(n.author_pubkey.len())]
                        ))
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
        "login-block" => gallery_misc::login_block(),
        _ => text("Unknown component").into(),
    }
}

fn claim_and_resolve_author(app: &GalleryApp, author_pubkey: &str) -> String {
    if author_pubkey.is_empty() {
        return String::new();
    }
    app.bridge.resolve_profile(author_pubkey, CONSUMER_ID);
    app.profiles.resolve(author_pubkey).display().to_string()
}

fn render_embed<'a, F>(
    nodes: &'a [WireNode],
    host: &'a nmp_gallery_tui::embed_host::EmbedHostState,
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
