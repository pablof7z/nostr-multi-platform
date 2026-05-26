use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::{data::GalleryData, render};

pub const COMPONENTS: &[&str] = &[
    "user-avatar",
    "user-name",
    "user-nip05",
    "user-npub",
    "user-card",
    "content-core",
    "content-view",
    "content-mention-chip",
    "content-minimal",
    "content-media-grid",
    "content-quote-card",
];

#[derive(Clone, Copy)]
pub struct ComponentSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

pub struct RegistrySectionSpec {
    pub label: &'static str,
    pub components: &'static [ComponentSpec],
}

const USER_COMPONENTS: &[ComponentSpec] = &[
    ComponentSpec {
        id: "user-avatar",
        label: "NostrAvatar",
        description: "Live kind:0 picture with identicon fallback",
    },
    ComponentSpec {
        id: "user-name",
        label: "NostrProfileName",
        description: "Display name with npub fallback",
    },
    ComponentSpec {
        id: "user-nip05",
        label: "NostrNip05Badge",
        description: "NIP-05 verified identity badge",
    },
    ComponentSpec {
        id: "user-npub",
        label: "NostrNpubChip",
        description: "Rust-truncated npub identity chip",
    },
    ComponentSpec {
        id: "user-card",
        label: "NostrUserCard",
        description: "Compact avatar, name, and NIP-05 row",
    },
];

const CONTENT_COMPONENTS: &[ComponentSpec] = &[
    ComponentSpec {
        id: "content-core",
        label: "ContentTreeWire",
        description: "Wire tree decoded from live event content",
    },
    ComponentSpec {
        id: "content-view",
        label: "NostrContentView",
        description: "Full rich content renderer",
    },
    ComponentSpec {
        id: "content-mention-chip",
        label: "NostrMentionChip",
        description: "Resolved @mention with deterministic color",
    },
    ComponentSpec {
        id: "content-minimal",
        label: "NostrMinimalContent",
        description: "Inline text, mentions, links, and hashtags",
    },
    ComponentSpec {
        id: "content-media-grid",
        label: "NostrMediaGrid",
        description: "Inline media projected from content",
    },
    ComponentSpec {
        id: "content-quote-card",
        label: "NostrQuoteCard",
        description: "Embedded event quote card",
    },
];

const EMBED_COMPONENTS: &[ComponentSpec] = &[
    ComponentSpec {
        id: "embed-article",
        label: "Embedded Article",
        description: "Real kind:30023 referenced inside surrounding text (card preview)",
    },
    ComponentSpec {
        id: "embed-profile",
        label: "Embedded Profile",
        description: "nostr:npub mention rendered inline",
    },
    ComponentSpec {
        id: "embed-note",
        label: "Embedded Note",
        description: "kind:1 nevent as a block card with proper content",
    },
    ComponentSpec {
        id: "embed-highlight",
        label: "Embedded Highlight",
        description: "NIP-84 highlight as a styled embed",
    },
];

pub const REGISTRY_SECTIONS: &[RegistrySectionSpec] = &[
    RegistrySectionSpec {
        label: "User",
        components: USER_COMPONENTS,
    },
    RegistrySectionSpec {
        label: "Content",
        components: CONTENT_COMPONENTS,
    },
    RegistrySectionSpec {
        label: "Embeds & Kinds",
        components: EMBED_COMPONENTS,
    },
];

pub struct GalleryView<'a> {
    selected_index: usize,
    data: &'a GalleryData,
}

impl<'a> GalleryView<'a> {
    pub fn new(selected_index: usize, data: &'a GalleryData) -> Self {
        Self {
            selected_index,
            data,
        }
    }
}

impl Widget for GalleryView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let outer = Block::default()
            .title(Line::from(vec![
                Span::styled(
                    "NmpGallery TUI",
                    Style::default().fg(Color::Rgb(125, 211, 252)),
                ),
                Span::raw(" / registry"),
            ]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(51, 65, 85)));
        let inner = outer.inner(area);
        outer.render(area, buf);

        let chunks = Layout::horizontal([Constraint::Length(34), Constraint::Min(0)])
            .spacing(1)
            .split(inner);
        render_sidebar(chunks[0], self.selected_index, buf);
        render_detail(component_at(self.selected_index), chunks[1], self.data, buf);
    }
}

pub fn is_component(id: &str) -> bool {
    COMPONENTS.contains(&id)
}

pub fn component_count() -> usize {
    REGISTRY_SECTIONS
        .iter()
        .map(|section| section.components.len())
        .sum()
}

pub fn component_index(id: &str) -> usize {
    REGISTRY_SECTIONS
        .iter()
        .flat_map(|section| section.components)
        .position(|component| component.id == id)
        .unwrap_or(0)
}

pub fn component_at(index: usize) -> ComponentSpec {
    REGISTRY_SECTIONS
        .iter()
        .flat_map(|section| section.components)
        .nth(index.min(component_count().saturating_sub(1)))
        .copied()
        .unwrap_or(USER_COMPONENTS[0])
}

fn render_sidebar(area: Rect, selected_index: usize, buf: &mut Buffer) {
    let selected = component_at(selected_index).id;
    let mut rows = Vec::new();
    for section in REGISTRY_SECTIONS {
        rows.push(Line::from(Span::styled(
            section.label,
            Style::default()
                .fg(Color::Rgb(125, 211, 252))
                .add_modifier(Modifier::BOLD),
        )));
        for component in section.components {
            let active = component.id == selected;
            let style = if active {
                Style::default()
                    .fg(Color::Rgb(248, 250, 252))
                    .bg(Color::Rgb(30, 41, 59))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(203, 213, 225))
            };
            rows.push(Line::from(vec![
                Span::styled(if active { "› " } else { "  " }, style),
                Span::styled(component.label, style),
            ]));
        }
        rows.push(Line::from(""));
    }

    let block = Block::default()
        .title("Components")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(51, 65, 85)));
    Paragraph::new(rows).block(block).render(area, buf);
}

fn render_detail(component: ComponentSpec, area: Rect, data: &GalleryData, buf: &mut Buffer) {
    let block = Block::default()
        .title(component.label)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(51, 65, 85)));
    let inner = block.inner(area);
    block.render(area, buf);

    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)])
        .spacing(1)
        .split(inner);
    component_header(component).render(chunks[0], buf);
    render::render_body(component.id, chunks[1], buf, data);
}

fn component_header(component: ComponentSpec) -> Paragraph<'static> {
    Paragraph::new(vec![
        Line::from(component.description),
        Line::from(Span::styled(
            format!("component: tui/{}", component.id),
            Style::default().fg(Color::Rgb(148, 163, 184)),
        )),
    ])
}
