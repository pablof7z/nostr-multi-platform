use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;

use egui::{Color32, Frame, Stroke, TextureHandle, Ui};

use nmp_content::embed_projection::{ArticleProjection, EmbedKindProjection, EmbeddedEventEnvelope};
use nmp_content::wire::{ContentTreeWire, WireNode};

/// Global texture cache for article hero images (URL → egui TextureHandle).
static IMAGE_CACHE: Mutex<Option<HashMap<String, TextureHandle>>> = Mutex::new(None);

fn get_cached_texture(ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
    {
        let lock = IMAGE_CACHE.lock().ok()?;
        if let Some(ref map) = *lock {
            if let Some(handle) = map.get(url) {
                return Some(handle.clone());
            }
        }
    }
    let texture = fetch_and_load_texture(ctx, url)?;
    {
        let mut lock = IMAGE_CACHE.lock().ok()?;
        let map = lock.get_or_insert_with(HashMap::new);
        map.insert(url.to_string(), texture.clone());
    }
    Some(texture)
}

fn fetch_and_load_texture(ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(8))
        .build();
    let response = agent.get(url).call().ok()?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(8 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let pixels = img.into_raw();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
    Some(ctx.load_texture(
        url,
        std::sync::Arc::new(color_image),
        egui::TextureOptions::LINEAR,
    ))
}

/// Article embed card for kind:30023.
///
/// Mirrors the TUI `DefaultArticleRenderer`: rounded box, bold title,
/// ● author · date · N min read byline, summary. When a hero image is
/// present the card uses a horizontal medium layout (image left, text
/// right); otherwise it falls back to the vertical stacked layout.
pub struct ArticleCard<'a> {
    article: &'a ArticleProjection,
}

impl<'a> ArticleCard<'a> {
    #[must_use]
    pub fn new(article: &'a ArticleProjection) -> Self {
        Self { article }
    }

    pub fn show(self, ui: &mut Ui) {
        let bg = ui.visuals().faint_bg_color;
        let stroke = Stroke::new(1.0, Color32::from_rgb(71, 85, 105));
        let padding = 10.0;

        Frame::group(ui.style())
            .fill(bg)
            .stroke(stroke)
            .inner_margin(egui::Margin::symmetric(padding, padding))
            .show(ui, |ui| {
                let has_image = self.article.hero_image_url.is_some();
                if has_image {
                    self.show_horizontal(ui, padding);
                } else {
                    self.show_vertical(ui, padding);
                }
            });
    }

    fn show_horizontal(&self, ui: &mut Ui, _padding: f32) {
        let image_size = 80.0;
        ui.horizontal(|ui| {
            // Left: hero image
            if let Some(ref url) = self.article.hero_image_url {
                if let Some(texture) = get_cached_texture(ui.ctx(), url) {
                    ui.image((texture.id(), egui::vec2(image_size, image_size)))
                        .on_hover_text(url.as_str());
                } else {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(image_size, image_size),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(
                        rect,
                        egui::Rounding::same(4.0),
                        Color32::from_rgb(51, 65, 85),
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "img",
                        egui::FontId::proportional(10.0),
                        Color32::from_rgb(148, 163, 184),
                    );
                }
                ui.add_space(10.0);
            }

            // Right: text content
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - image_size - 10.0);
                self.show_text_content(ui);
            });
        });
    }

    fn show_vertical(&self, ui: &mut Ui, _padding: f32) {
        self.show_text_content(ui);
    }

    fn show_text_content(&self, ui: &mut Ui) {
        let title = self.article.title.as_deref().unwrap_or("article");
        ui.label(
            egui::RichText::new(truncate_chars(title, 200))
                .strong()
                .size(15.0)
                .color(Color32::from_rgb(241, 245, 249)),
        );
        ui.add_space(3.0);

        let author = self
            .article
            .author_display_name
            .as_deref()
            .unwrap_or_else(|| &self.article.author_pubkey[..8.min(self.article.author_pubkey.len())]);
        let short_date = format_short_date(self.article.created_at);
        let summary_text: String = self
            .article
            .summary
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| tree_text(&self.article.content_tree));
        let reading_min = estimate_reading_time(title, &summary_text);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("\u{25CF}")
                    .color(Color32::from_rgb(220, 38, 38))
                    .size(9.0),
            );
            ui.label(
                egui::RichText::new(author)
                    .size(11.0)
                    .color(Color32::from_rgb(203, 213, 225)),
            );
            ui.label(
                egui::RichText::new(format!("\u{00B7} {short_date}"))
                    .size(11.0)
                    .color(Color32::from_rgb(100, 116, 139)),
            );
            ui.label(
                egui::RichText::new(format!("\u{00B7} {reading_min} min read"))
                    .size(11.0)
                    .color(Color32::from_rgb(100, 116, 139)),
            );
        });
        ui.add_space(4.0);

        ui.label(
            egui::RichText::new(truncate_chars(&summary_text, 300))
                .size(12.0)
                .color(Color32::from_rgb(148, 163, 184)),
        );
    }
}

/// Try to render an article from the envelope map keyed by `primary_id`.
pub fn try_render_article(
    ui: &mut Ui,
    primary_id: &str,
    envelopes: &std::collections::BTreeMap<String, EmbeddedEventEnvelope>,
) -> bool {
    let Some(envelope) = envelopes.get(primary_id) else {
        return false;
    };
    let EmbedKindProjection::Article(ref article) = envelope.projection else {
        return false;
    };
    ArticleCard::new(article).show(ui);
    true
}

fn truncate_chars(s: &str, max: usize) -> String {
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

fn format_short_date(unix_secs: u64) -> String {
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
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
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

fn estimate_reading_time(title: &str, summary: &str) -> u32 {
    let words = title.split_whitespace().count() + summary.split_whitespace().count();
    let estimated_words = (words * 10).max(200);
    ((estimated_words as f32 / 200.0).ceil() as u32).max(1)
}

pub fn tree_text(tree: &ContentTreeWire) -> String {
    let mut out = Vec::new();
    for root in &tree.roots {
        let idx = *root as usize;
        if let Some(node) = tree.nodes.get(idx) {
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
        WireNode::Url { url } => url.clone(),
        WireNode::Media { urls, .. } => format!("[media: {} urls]", urls.len()),
        WireNode::Emoji { shortcode, .. } => format!(":{shortcode}:"),
        WireNode::Invoice { .. } => "[invoice]".to_string(),
        WireNode::Paragraph { children }
        | WireNode::Emphasis { children }
        | WireNode::Strong { children }
        | WireNode::BlockQuote { children } => children_text(tree, children),
        WireNode::Heading { children, .. } => children_text(tree, children),
        WireNode::Link { children, href } => {
            if let Some(href) = href {
                href.clone()
            } else {
                children_text(tree, children)
            }
        }
        WireNode::Image { alt, src, .. } => {
            if let Some(src) = src {
                src.clone()
            } else {
                alt.clone()
            }
        }
        WireNode::CodeBlock { info, body } => {
            if let Some(info) = info {
                format!("```{info}\n{body}\n```")
            } else {
                format!("```\n{body}\n```")
            }
        }
        WireNode::List { items, .. } => {
            items
                .iter()
                .filter_map(|item| {
                    let text = children_text(tree, item);
                    if text.is_empty() {
                        None
                    } else {
                        Some(text)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        WireNode::InlineCode { code } => code.clone(),
        WireNode::SoftBreak | WireNode::HardBreak | WireNode::Rule => String::new(),
        WireNode::Placeholder { reason } => format!("[placeholder: {reason:?}]"),
    }
}

fn children_text(tree: &ContentTreeWire, children: &[u32]) -> String {
    children
        .iter()
        .filter_map(|c| {
            let idx = *c as usize;
            tree.nodes.get(idx).map(|n| node_text(tree, n))
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn short_id(id: &str) -> String {
    if id.len() > 16 {
        format!("{}\u{2026}{}", &id[..8], &id[id.len() - 8..])
    } else {
        id.to_string()
    }
}
