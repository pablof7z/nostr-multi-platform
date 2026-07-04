//! Shared terminal text-layout primitives for the `content-kind-registry`
//! renderers (kind:1 short-note, kind:30023 article, kind:9802 highlight,
//! kind:0 profile, and the unknown-kind fallback): timestamp formatting,
//! content-tree flattening, truncation, and wrapped-line height estimation.
//! Not owned by any single kind — analogous to `kind_renderer::author_byline`.

use nmp_content::wire::{ContentTreeWire, WireNode};

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

pub(super) fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let mut out: String = chars.iter().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

pub(super) fn format_relative_time(unix_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let delta = now.saturating_sub(unix_secs);

    if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86400 {
        format!("{}h ago", delta / 3600)
    } else if delta < 30 * 86400 {
        format!("{}d ago", delta / 86400)
    } else {
        format_short_date(unix_secs)
    }
}

pub(super) fn format_short_date(unix_secs: u64) -> String {
    // Days since Unix epoch → calendar date (Gregorian, no external crate).
    let days = unix_secs / 86400;
    let mut y = 1970u32;
    let mut d = days as u32;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31u32,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut month = 0usize;
    while month < 12 && d >= month_days[month] {
        d -= month_days[month];
        month += 1;
    }
    format!("{} {}", month_names[month.min(11)], d + 1)
}

pub(super) fn estimate_reading_time(title: &str, summary: &str) -> u32 {
    let words = title.split_whitespace().count() + summary.split_whitespace().count();
    // Assume full article is ~10× the summary word count; 200 wpm average.
    let estimated_words = (words * 10).max(200);
    ((estimated_words as f32 / 200.0).ceil() as u32).max(1)
}

pub(super) fn render_two_line(
    header: &str,
    body: &str,
    area: ratatui::layout::Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let lines = vec![
        Line::from(Span::styled(
            header.to_string(),
            Style::default().fg(Color::Rgb(148, 163, 184)),
        )),
        Line::from(Span::raw(body.to_string())),
    ];
    Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .render(area, buf);
}

pub(super) fn tree_text(tree: &ContentTreeWire) -> String {
    let mut out = Vec::new();
    for root in &tree.roots {
        if let Some(node) = tree.nodes.get(*root as usize) {
            let text = node_text(tree, node);
            if !text.is_empty() {
                out.push(text);
            }
        }
    }
    out.join("\n")
}

fn node_text(tree: &ContentTreeWire, node: &WireNode) -> String {
    match node {
        WireNode::Text { text } => text.clone(),
        WireNode::Mention { uri } => format!("@{}", short_id(&uri.primary_id)),
        WireNode::EventRef { uri } => format!("nostr:{}", short_id(&uri.primary_id)),
        WireNode::Hashtag { tag } => format!("#{tag}"),
        WireNode::Url { url } | WireNode::AdCandidateUrl { url } => url.clone(),
        WireNode::Media { urls, media_kind } => {
            format!("[{:?} media: {}]", media_kind, urls.len())
        }
        WireNode::Emoji { shortcode, .. } => format!(":{shortcode}:"),
        WireNode::Invoice { .. } => "[invoice]".to_string(),
        WireNode::Heading { children, .. }
        | WireNode::Paragraph { children }
        | WireNode::BlockQuote { children }
        | WireNode::Emphasis { children }
        | WireNode::Strong { children }
        | WireNode::Link { children, .. } => children_text(tree, children),
        WireNode::CodeBlock { body, .. } => body.clone(),
        WireNode::List {
            ordered_start,
            items,
        } => items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let marker = ordered_start
                    .map(|start| format!("{}.", start + idx as u64))
                    .unwrap_or_else(|| "-".to_string());
                format!("{marker} {}", children_text(tree, item))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        WireNode::InlineCode { code } => format!("`{code}`"),
        WireNode::Image { alt, src, .. } => src
            .as_deref()
            .map(|src| format!("[image: {alt} {src}]"))
            .unwrap_or_else(|| format!("[image: {alt}]")),
        WireNode::SoftBreak => " ".to_string(),
        WireNode::HardBreak => "\n".to_string(),
        WireNode::Rule => String::new(),
        WireNode::Placeholder { reason } => format!("[{reason:?}]"),
    }
}

fn children_text(tree: &ContentTreeWire, children: &[u32]) -> String {
    children
        .iter()
        .filter_map(|idx| tree.nodes.get(*idx as usize))
        .map(|node| node_text(tree, node))
        .collect::<Vec<_>>()
        .join("")
}

pub(super) fn text_height(body: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    body.lines()
        .map(|line| (line.chars().count().max(1).saturating_add(width - 1) / width) as u16)
        .sum::<u16>()
        .max(1)
}

fn short_id(hex: &str) -> String {
    if hex.chars().count() > 8 {
        hex.chars().take(8).collect()
    } else {
        hex.to_string()
    }
}
