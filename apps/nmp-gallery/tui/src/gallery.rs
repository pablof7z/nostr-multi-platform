use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use nmp_app_gallery::registry::{
    registry, ComponentSpec as RegistryComponentSpec, RegistrySection,
};

use crate::{data::GalleryData, render, render::EmbedFrameContext};

#[derive(Clone, Copy)]
pub struct ComponentSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

pub struct GalleryView<'a> {
    selected_index: usize,
    data: &'a GalleryData,
    embed_ctx: EmbedFrameContext<'a>,
}

impl<'a> GalleryView<'a> {
    pub fn new(
        selected_index: usize,
        data: &'a GalleryData,
        embed_ctx: EmbedFrameContext<'a>,
    ) -> Self {
        Self {
            selected_index,
            data,
            embed_ctx,
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
        render_detail(
            component_at(self.selected_index),
            chunks[1],
            self.data,
            buf,
            self.embed_ctx,
        );
    }
}

pub fn registry_sections() -> &'static [RegistrySection] {
    &registry().sections
}

fn component_spec(component: &'static RegistryComponentSpec) -> ComponentSpec {
    ComponentSpec {
        id: component.id.as_str(),
        label: component.label.as_str(),
        description: component.description.as_str(),
    }
}

fn component_specs() -> impl Iterator<Item = ComponentSpec> {
    registry_sections()
        .iter()
        .flat_map(|section| section.components.iter())
        .map(component_spec)
}

pub fn component_ids() -> impl Iterator<Item = &'static str> {
    component_specs().map(|component| component.id)
}

pub fn is_component(id: &str) -> bool {
    component_specs().any(|component| component.id == id)
}

pub fn component_count() -> usize {
    registry_sections()
        .iter()
        .map(|section| section.components.len())
        .sum()
}

pub fn component_index(id: &str) -> usize {
    component_specs()
        .position(|component| component.id == id)
        .unwrap_or(0)
}

pub fn component_at(index: usize) -> ComponentSpec {
    component_specs()
        .nth(index.min(component_count().saturating_sub(1)))
        .or_else(|| component_specs().next())
        .expect("registry.json must contain at least one component")
}

fn render_sidebar(area: Rect, selected_index: usize, buf: &mut Buffer) {
    let selected = component_at(selected_index).id;
    let mut rows = Vec::new();
    for section in registry_sections() {
        rows.push(Line::from(Span::styled(
            section.label.as_str(),
            Style::default()
                .fg(Color::Rgb(125, 211, 252))
                .add_modifier(Modifier::BOLD),
        )));
        for component in &section.components {
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
                Span::styled(component.label.as_str(), style),
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

fn render_detail(
    component: ComponentSpec,
    area: Rect,
    data: &GalleryData,
    buf: &mut Buffer,
    embed_ctx: EmbedFrameContext<'_>,
) {
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
    render::render_body(component.id, chunks[1], buf, data, embed_ctx);
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
