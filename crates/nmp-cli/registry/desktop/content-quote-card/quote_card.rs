use iced::widget::{column, container, row, text};
use iced::{Border, Element, Length};

use nmp_content::embed_projection::{EmbedKindProjection, EmbeddedEventEnvelope};
use nmp_content::wire::{ContentTreeWire, WireNode};
use nmp_gallery_tui::content_render_data::ContentEventRenderData;

use super::content_core::{short_id, BORDER_COLOR, MUTED, SURFACE, TEXT};

pub struct NostrQuoteCard {
    author: String,
    content: String,
    meta: String,
    missing: bool,
}

impl NostrQuoteCard {
    #[must_use]
    pub fn from_event(event: &ContentEventRenderData) -> Self {
        Self {
            author: event.author_label().to_string(),
            content: event.content_preview.clone(),
            meta: format!("kind:{} · {}", event.kind, short_id(&event.id)),
            missing: false,
        }
    }

    #[must_use]
    pub fn from_envelope(envelope: &EmbeddedEventEnvelope) -> Option<Self> {
        match &envelope.projection {
            EmbedKindProjection::ShortNote(note) => Some(Self {
                author: short_id(&note.author_pubkey),
                content: projection_preview(&note.content_tree),
                meta: format!("kind:1 · {}", short_id(&note.id)),
                missing: false,
            }),
            EmbedKindProjection::Unknown(unknown) => Some(Self {
                author: short_id(&unknown.author_pubkey),
                content: unknown.content.clone(),
                meta: format!("kind:{}", unknown.kind),
                missing: false,
            }),
            _ => None,
        }
    }

    #[must_use]
    pub fn missing(id: &str) -> Self {
        Self {
            author: "Resolving event".to_string(),
            content: "Waiting for the relay-backed event projection.".to_string(),
            meta: short_id(id),
            missing: true,
        }
    }

    pub fn into_element<Message: 'static>(self) -> Element<'static, Message> {
        let accent = if self.missing { MUTED } else { TEXT };
        let body = column![
            row![
                text(self.author).size(13).style(move |_| text::Style {
                    color: Some(accent),
                }),
                text(self.meta)
                    .size(11)
                    .style(|_| text::Style { color: Some(MUTED) })
            ]
            .spacing(8),
            text(self.content)
                .size(12)
                .style(|_| text::Style { color: Some(MUTED) })
        ]
        .spacing(6);

        container(body)
            .width(Length::Fill)
            .padding(10)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(SURFACE)),
                border: Border {
                    color: BORDER_COLOR,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}

fn projection_preview(tree: &ContentTreeWire) -> String {
    let roots = if tree.roots.is_empty() {
        (0..tree.nodes.len())
            .filter_map(|idx| u32::try_from(idx).ok())
            .collect::<Vec<_>>()
    } else {
        tree.roots.clone()
    };
    let mut preview = roots
        .iter()
        .filter_map(|idx| tree.nodes.get(*idx as usize))
        .map(|node| node_text(tree, node))
        .collect::<Vec<_>>()
        .join(" ");
    preview.truncate(180);
    if preview.trim().is_empty() {
        "Resolved note".to_string()
    } else {
        preview
    }
}

fn node_text(tree: &ContentTreeWire, node: &WireNode) -> String {
    match node {
        WireNode::Text { text } => text.clone(),
        WireNode::Paragraph { children }
        | WireNode::Heading { children, .. }
        | WireNode::BlockQuote { children }
        | WireNode::Emphasis { children }
        | WireNode::Strong { children }
        | WireNode::Link { children, .. } => children
            .iter()
            .filter_map(|idx| tree.nodes.get(*idx as usize))
            .map(|child| node_text(tree, child))
            .collect::<Vec<_>>()
            .join(""),
        WireNode::List { items, .. } => items
            .iter()
            .map(|item| {
                item.iter()
                    .filter_map(|idx| tree.nodes.get(*idx as usize))
                    .map(|child| node_text(tree, child))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect::<Vec<_>>()
            .join(" "),
        WireNode::Mention { uri } => format!("@{}", short_id(&uri.primary_id)),
        WireNode::EventRef { uri } => format!("nostr:{}", short_id(&uri.primary_id)),
        WireNode::Hashtag { tag } => format!("#{tag}"),
        WireNode::Url { url } => url.clone(),
        WireNode::InlineCode { code } => code.clone(),
        WireNode::CodeBlock { body, .. } => body.clone(),
        WireNode::Image { alt, .. } => alt.clone(),
        WireNode::Media { urls, .. } => format!("{} media item(s)", urls.len()),
        WireNode::Emoji { shortcode, .. } => format!(":{shortcode}:"),
        WireNode::Invoice { invoice } => format!("{invoice:?}"),
        WireNode::SoftBreak | WireNode::HardBreak => " ".to_string(),
        WireNode::Rule => String::new(),
        WireNode::Placeholder { .. } => "unresolved content".to_string(),
    }
}
