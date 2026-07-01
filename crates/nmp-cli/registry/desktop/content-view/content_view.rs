use std::collections::{BTreeMap, BTreeSet};

use iced::widget::{column, row, text};
use iced::Element;

use nmp_content::embed_projection::{EmbedKindProjection, EmbeddedEventEnvelope};
use nmp_gallery_tui::content_render_data::ContentRenderData;
use nmp_gallery_tui::content_tree_wire::{ContentTreeWire, WireNode, WireUri};

use super::content_core::{event_refs, framed, inline_label, media_urls, pill};
use super::media_grid::NostrMediaGrid;
use super::mention_chip::NostrMentionChip;
use super::quote_card::NostrQuoteCard;

pub struct NostrContentView<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
    embedded_events: Option<&'a BTreeMap<String, EmbeddedEventEnvelope>>,
    profile_labels: BTreeMap<String, String>,
    media_handles: &'a BTreeMap<String, iced::widget::image::Handle>,
}

impl<'a> NostrContentView<'a> {
    #[must_use]
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self {
            tree,
            render_data: None,
            embedded_events: None,
            profile_labels: BTreeMap::new(),
            media_handles: &EMPTY_HANDLES,
        }
    }

    #[must_use]
    pub fn render_data(mut self, render_data: Option<&'a ContentRenderData>) -> Self {
        self.render_data = render_data;
        self
    }

    #[must_use]
    pub fn embedded_events(
        mut self,
        embedded_events: Option<&'a BTreeMap<String, EmbeddedEventEnvelope>>,
    ) -> Self {
        self.embedded_events = embedded_events;
        self
    }

    #[must_use]
    pub fn profile_labels(mut self, profile_labels: BTreeMap<String, String>) -> Self {
        self.profile_labels = profile_labels;
        self
    }

    #[must_use]
    pub fn media_handles(
        mut self,
        media_handles: &'a BTreeMap<String, iced::widget::image::Handle>,
    ) -> Self {
        self.media_handles = media_handles;
        self
    }

    pub fn into_element<Message: 'static>(self) -> Element<'a, Message> {
        let mut body = column![].spacing(8);
        let roots = if self.tree.roots.is_empty() {
            (0..self.tree.nodes.len()).collect()
        } else {
            self.tree.roots.clone()
        };
        let mut rendered = BTreeSet::new();

        for idx in roots {
            if let Some(node) = self.tree.node(idx) {
                rendered.insert(idx);
                body = body.push(render_node(&self, node));
            }
        }

        for (idx, node) in self.tree.nodes.iter().enumerate() {
            if !rendered.contains(&idx)
                && matches!(
                    node,
                    WireNode::EventRef(_) | WireNode::Media { .. } | WireNode::Image { .. }
                )
            {
                body = body.push(render_node(&self, node));
            }
        }

        framed(body)
    }
}

static EMPTY_HANDLES: BTreeMap<String, iced::widget::image::Handle> = BTreeMap::new();

fn render_node<'a, Message: 'static>(
    view: &NostrContentView<'a>,
    node: &'a WireNode,
) -> Element<'a, Message> {
    match node {
        WireNode::Paragraph { children } => render_inline(view, children),
        WireNode::Heading { children, .. } => {
            text(inline_label(view.tree, children)).size(16).into()
        }
        WireNode::Mention(uri) => render_mention(view, uri),
        WireNode::EventRef(uri) => render_event_ref(view, uri),
        WireNode::Media { urls, .. } => NostrMediaGrid::new(urls)
            .image_handles(view.media_handles)
            .into_element(),
        WireNode::Image { src: Some(src), .. } => NostrMediaGrid::new(std::slice::from_ref(src))
            .image_handles(view.media_handles)
            .into_element(),
        WireNode::Text(value) => text(value.clone()).size(13).into(),
        _ => text(node.inline_label(view.tree)).size(13).into(),
    }
}

fn render_inline<'a, Message: 'static>(
    view: &NostrContentView<'a>,
    children: &'a [usize],
) -> Element<'a, Message> {
    let mut out = row![].spacing(6);
    for child in children {
        if let Some(node) = view.tree.node(*child) {
            out = out.push(match node {
                WireNode::Mention(uri) => render_mention(view, uri),
                WireNode::EventRef(uri) => render_event_ref(view, uri),
                WireNode::Text(value) if !value.trim().is_empty() => {
                    text(value.clone()).size(13).into()
                }
                WireNode::Url(url) => pill(url.clone()),
                WireNode::Hashtag(tag) => pill(format!("#{tag}")),
                _ => text(node.inline_label(view.tree)).size(13).into(),
            });
        }
    }
    out.into()
}

fn render_mention<'a, Message: 'static>(
    view: &NostrContentView<'a>,
    uri: &'a WireUri,
) -> Element<'a, Message> {
    let mut chip =
        NostrMentionChip::new(uri).profile(view.render_data.and_then(|data| data.profile_for(uri)));
    if let Some(label) = view.profile_labels.get(&uri.primary_id) {
        chip = chip.label(label.clone());
    }
    chip.into_element()
}

fn render_event_ref<'a, Message: 'static>(
    view: &NostrContentView<'a>,
    uri: &'a WireUri,
) -> Element<'a, Message> {
    if let Some(event) = view.render_data.and_then(|data| data.event_for(uri)) {
        return NostrQuoteCard::from_event(event).into_element();
    }

    if let Some(envelope) = view
        .embedded_events
        .and_then(|events| events.get(&uri.primary_id))
    {
        if let Some(card) = NostrQuoteCard::from_envelope(envelope) {
            return card.into_element();
        }
        if let EmbedKindProjection::Article(article) = &envelope.projection {
            let urls = article
                .hero_image_url
                .as_ref()
                .filter(|url| !url.is_empty())
                .map(|url| vec![url.clone()])
                .unwrap_or_default();
            return column![
                text(article.title.as_deref().unwrap_or("Article")).size(14),
                NostrMediaGrid::new(&urls)
                    .image_handles(view.media_handles)
                    .into_element()
            ]
            .spacing(8)
            .into();
        }
    }

    NostrQuoteCard::missing(&uri.primary_id).into_element()
}

pub fn referenced_media_urls(
    tree: &ContentTreeWire,
    embedded_events: &BTreeMap<String, EmbeddedEventEnvelope>,
) -> Vec<String> {
    let mut urls = media_urls(tree);
    for uri in event_refs(tree) {
        if let Some(envelope) = embedded_events.get(&uri.primary_id) {
            match &envelope.projection {
                EmbedKindProjection::Article(article) => {
                    if let Some(url) = article
                        .hero_image_url
                        .as_ref()
                        .filter(|url| !url.is_empty())
                    {
                        urls.push(url.clone());
                    }
                }
                EmbedKindProjection::ShortNote(note) => {
                    urls.extend(note.media_urls.iter().cloned())
                }
                _ => {}
            }
        }
    }
    urls.sort();
    urls.dedup();
    urls
}
