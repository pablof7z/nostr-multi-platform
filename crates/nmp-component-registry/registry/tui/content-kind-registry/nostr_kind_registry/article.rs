//! Default renderer for kind:30023 — long-form articles (NIP-23), the
//! `ArticleProjection` variant of `EmbedKindProjection`.

use nmp_content::embed_projection::EmbedKindProjection;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

use super::super::super::nostr_mention_chip::NostrMentionProfileHost;
use super::super::kind_renderer::{author_byline, KindRenderer};
use super::text_layout::{estimate_reading_time, format_short_date, tree_text, truncate_chars};
use super::NostrKindRegistry;

/// Default renderer for `ArticleProjection` (kind:30023).
/// Continuous-byline card: rounded box, bold title, `● author · date · N min read`, summary.
pub struct DefaultArticleRenderer;

impl KindRenderer for DefaultArticleRenderer {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        _ctx: &nmp_content::context::RenderContext,
        _registry: &NostrKindRegistry,
        host: Option<&dyn NostrMentionProfileHost>,
        consumer_id: Option<&str>,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::Article(article) = projection else {
            return;
        };
        if area.height < 5 || area.width < 6 {
            return;
        }

        // Component-owned kind:0: self-claiming author byline (iOS PR #833).
        let author = author_byline(host, consumer_id, &article.author_pubkey);
        let title = article.title.as_deref().unwrap_or("article");
        let summary = article
            .summary
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| tree_text(&article.content_tree));
        let short_date = format_short_date(article.created_at);
        let reading_min = estimate_reading_time(title, &summary);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(71, 85, 105)));
        let inner = block.inner(area);
        block.render(area, buf);

        let content = Rect {
            x: inner.x + 1,
            y: inner.y,
            width: inner.width.saturating_sub(1),
            height: inner.height,
        };
        if content.width == 0 || content.height == 0 {
            return;
        }

        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(content);

        // Title
        let title_str = truncate_chars(title, content.width as usize);
        Paragraph::new(Line::from(Span::styled(
            title_str,
            Style::default()
                .fg(Color::Rgb(241, 245, 249))
                .add_modifier(Modifier::BOLD),
        )))
        .render(rows[0], buf);

        // Byline: ● Author · Date · N min read
        let meta = format!(" \u{00B7} {} \u{00B7} {} min read", short_date, reading_min);
        Paragraph::new(Line::from(vec![
            Span::styled("\u{25CF} ", Style::default().fg(Color::Rgb(220, 38, 38))),
            Span::styled(author, Style::default().fg(Color::Rgb(203, 213, 225))),
            Span::styled(meta, Style::default().fg(Color::Rgb(100, 116, 139))),
        ]))
        .render(rows[1], buf);

        // Summary
        let summary_str = truncate_chars(&summary, content.width as usize);
        Paragraph::new(Line::from(Span::styled(
            summary_str,
            Style::default().fg(Color::Rgb(148, 163, 184)),
        )))
        .render(rows[2], buf);
    }

    fn preferred_height(&self, _projection: &EmbedKindProjection, _width: u16) -> u16 {
        5
    }
}
