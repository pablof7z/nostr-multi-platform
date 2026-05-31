use iced::widget::{column, container, row, text};
use iced::{Border, Color, Element, Length};

use nmp_content::embed_projection::ArticleProjection;

const MUTED: Color = Color { r: 0.580, g: 0.639, b: 0.722, a: 1.0 };
const FAINT_BG: Color = Color { r: 0.071, g: 0.098, b: 0.141, a: 1.0 };
const BORDER_COLOR: Color = Color { r: 0.278, g: 0.333, b: 0.404, a: 1.0 };

/// Iced article embed card for kind:30023.
///
/// Shows title, author · date · N min read byline, and summary. No image
/// loading for now — hero image is a follow-on.
///
/// Component-owned claiming (mirrors iOS #833): the byline renders from an
/// `author_name` the *displaying* renderer resolved from a profile it claimed
/// (presentation-owned claiming), NOT from the projection's static
/// `author_display_name` field. The render path in `gallery.rs` claims the
/// article author's kind:0 and resolves it through `LiveProfileMap` before
/// constructing this card. The kernel still emits `author_display_name` for
/// now, but this component no longer depends on it for display.
pub struct ArticleCard<'a> {
    article: &'a ArticleProjection,
    /// Presentation-resolved author label: the displaying renderer's
    /// `ProfileWire::display()` (real display name, or npub_short fallback).
    author_name: String,
}

impl<'a> ArticleCard<'a> {
    /// Build a card from the article projection plus the author label the
    /// displaying renderer resolved from its own profile claim.
    #[must_use]
    pub fn new(article: &'a ArticleProjection, author_name: impl Into<String>) -> Self {
        Self {
            article,
            author_name: author_name.into(),
        }
    }

    pub fn into_element<Message: 'static>(self) -> Element<'a, Message> {
        let a = self.article;

        let title = a.title.as_deref().unwrap_or("Untitled article");
        let author = self.author_name;
        let date = format_short_date(a.created_at);

        let summary_src = a.summary.as_deref().unwrap_or_default();
        let snippet: String = summary_src.chars().take(300).collect();

        let title_row = text(title)
            .size(15)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::default()
            })
            .style(|_| iced::widget::text::Style {
                color: Some(Color::from_rgb8(241, 245, 249)),
            });

        let byline = row![
            text("●").size(9).style(|_| iced::widget::text::Style {
                color: Some(Color::from_rgb8(220, 38, 38)),
            }),
            text(author).size(11).style(|_| iced::widget::text::Style {
                color: Some(MUTED),
            }),
            text(format!("· {date}"))
                .size(11)
                .style(|_| iced::widget::text::Style {
                    color: Some(Color { r: 0.392, g: 0.455, b: 0.545, a: 1.0 }),
                }),
        ]
        .spacing(4);

        let summary_text = text(snippet).size(12).style(|_| iced::widget::text::Style {
            color: Some(MUTED),
        });

        let inner = column![title_row, byline, summary_text].spacing(6);

        container(inner)
            .width(Length::Fill)
            .padding(12)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(FAINT_BG)),
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

fn format_short_date(unix_secs: u64) -> String {
    // Simple ISO-like date without heavy dependencies.
    let days = unix_secs / 86400;
    let mut y = 1970u32;
    let mut d = days as u32;
    loop {
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31u32, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut month = 0usize;
    while month < 12 && d >= month_days[month] {
        d -= month_days[month];
        month += 1;
    }
    format!("{} {} {}", month_names[month.min(11)], d + 1, y)
}
