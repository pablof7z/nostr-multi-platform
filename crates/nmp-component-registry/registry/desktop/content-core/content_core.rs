use iced::widget::{column, container, text};
use iced::{Border, Color, Element, Length};

use nmp_gallery_tui::content_tree_wire::{ContentTreeWire, WireNode, WireUri};

pub const SURFACE: Color = Color {
    r: 0.071,
    g: 0.098,
    b: 0.141,
    a: 1.0,
};
pub const SURFACE_2: Color = Color {
    r: 0.118,
    g: 0.161,
    b: 0.231,
    a: 1.0,
};
pub const BORDER_COLOR: Color = Color {
    r: 0.278,
    g: 0.333,
    b: 0.404,
    a: 1.0,
};
pub const TEXT: Color = Color {
    r: 0.890,
    g: 0.922,
    b: 0.961,
    a: 1.0,
};
pub const MUTED: Color = Color {
    r: 0.580,
    g: 0.639,
    b: 0.722,
    a: 1.0,
};
pub const ACCENT: Color = Color {
    r: 0.490,
    g: 0.827,
    b: 0.988,
    a: 1.0,
};

pub struct ContentTreePanel<'a> {
    tree: &'a ContentTreeWire,
    title: Option<&'a str>,
}

impl<'a> ContentTreePanel<'a> {
    #[must_use]
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self { tree, title: None }
    }

    #[must_use]
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    pub fn into_element<Message: 'static>(self) -> Element<'a, Message> {
        let title = self.title.unwrap_or("ContentTreeWire");
        let mut body = column![text(title)
            .size(14)
            .style(|_| text::Style { color: Some(TEXT) })]
        .spacing(8);

        for idx in roots_or_all(self.tree) {
            if let Some(node) = self.tree.node(idx) {
                body = body.push(node_summary(self.tree, node));
            }
        }

        framed(body).into()
    }
}

pub fn inline_label(tree: &ContentTreeWire, children: &[usize]) -> String {
    let label = tree.inline_text(children);
    if label.trim().is_empty() {
        "content block".to_string()
    } else {
        label
    }
}

pub fn first_mention(tree: &ContentTreeWire) -> Option<&WireUri> {
    tree.nodes.iter().find_map(|node| match node {
        WireNode::Mention(uri) => Some(uri),
        _ => None,
    })
}

pub fn event_refs(tree: &ContentTreeWire) -> Vec<&WireUri> {
    tree.nodes
        .iter()
        .filter_map(|node| match node {
            WireNode::EventRef(uri) => Some(uri),
            _ => None,
        })
        .collect()
}

pub fn media_urls(tree: &ContentTreeWire) -> Vec<String> {
    tree.media_urls()
}

pub fn short_id(id: &str) -> String {
    let chars: String = id.chars().take(10).collect();
    if id.chars().count() > 10 {
        format!("{chars}...")
    } else {
        chars
    }
}

pub fn pill<'a, Message: 'static>(label: impl Into<String>) -> Element<'a, Message> {
    container(text(label.into()).size(12).style(|_| text::Style {
        color: Some(ACCENT),
    }))
    .padding([5, 8])
    .style(|_| container::Style {
        background: Some(iced::Background::Color(SURFACE_2)),
        border: Border {
            color: BORDER_COLOR,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn framed<'a, Message: 'static>(body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(body)
        .width(Length::Fill)
        .padding(12)
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

fn roots_or_all(tree: &ContentTreeWire) -> Vec<usize> {
    if tree.roots.is_empty() {
        (0..tree.nodes.len()).collect()
    } else {
        tree.roots.clone()
    }
}

fn node_summary<'a, Message: 'static>(
    tree: &ContentTreeWire,
    node: &WireNode,
) -> Element<'a, Message> {
    match node {
        WireNode::Paragraph { children } => text(inline_label(tree, children)).size(13),
        WireNode::Heading { level, children } => {
            text(format!("h{level} {}", inline_label(tree, children))).size(14)
        }
        WireNode::Mention(uri) => text(format!("@{}", short_id(&uri.primary_id))).size(13),
        WireNode::EventRef(uri) => text(format!("event {}", short_id(&uri.primary_id))).size(13),
        WireNode::Media { urls, kind } => text(format!("{kind} media x{}", urls.len())).size(13),
        WireNode::Image { alt, src, .. } => {
            text(format!("image {} {}", alt, src.as_deref().unwrap_or(""))).size(13)
        }
        WireNode::Text(value) => text(value.clone()).size(13),
        WireNode::Url(url) | WireNode::AdCandidateUrl(url) => text(url.clone()).size(13),
        WireNode::Hashtag(tag) => text(format!("#{tag}")).size(13),
        WireNode::BlockQuote { children } => {
            text(format!("quote {}", inline_label(tree, children))).size(13)
        }
        WireNode::CodeBlock { info, body } => text(format!(
            "code {} {}",
            info.as_deref().unwrap_or(""),
            body.lines().next().unwrap_or("")
        ))
        .size(13),
        WireNode::Rule => text("rule").size(13),
        _ => text(node.inline_label(tree)).size(13),
    }
    .style(|_| text::Style { color: Some(MUTED) })
    .into()
}
