//! Display helpers for `--smoke` mode: projection label, resolved-event
//! pretty-printer, and text truncation. Split from `main.rs` for file-size
//! compliance (500-LOC hard cap; `main.rs` baseline is 748).

use nmp_content::embed_projection::{EmbedKindProjection, EmbeddedEventEnvelope};

pub(crate) fn projection_label(p: &EmbedKindProjection) -> &'static str {
    match p {
        EmbedKindProjection::Article(_) => "Article (kind:30023)",
        EmbedKindProjection::ShortNote(_) => "ShortNote (kind:1)",
        EmbedKindProjection::Highlight(_) => "Highlight (kind:9802)",
        EmbedKindProjection::Profile(_) => "Profile (kind:0)",
        EmbedKindProjection::Unknown(_) => "Unknown",
    }
}

pub(crate) fn print_resolved(label: &str, env: &EmbeddedEventEnvelope) {
    match &env.projection {
        EmbedKindProjection::Article(a) => {
            println!("✓ {label} → ArticleProjection (kind:30023)");
            println!("    id:        {}", a.id);
            println!("    author:    {}", a.author_pubkey);
            println!("    d_tag:     {}", a.d_tag);
            if let Some(title) = &a.title {
                println!("    title:     {title}");
            }
            if let Some(summary) = &a.summary {
                println!("    summary:   {summary}");
            }
            if let Some(hero) = &a.hero_image_url {
                println!("    hero:      {hero}");
            }
        }
        EmbedKindProjection::ShortNote(n) => {
            println!("✓ {label} → ShortNoteProjection (kind:1)");
            println!("    id:        {}", n.id);
            println!("    author:    {}", n.author_pubkey);
            println!("    media:     {:?}", n.media_urls);
        }
        EmbedKindProjection::Highlight(h) => {
            println!("✓ {label} → HighlightProjection (kind:9802)");
            println!("    id:        {}", h.id);
            println!(
                "    quoted:    {}",
                truncate_for_display(&h.highlighted_text, 80)
            );
        }
        EmbedKindProjection::Profile(p) => {
            println!("✓ {label} → ProfileProjection (kind:0)");
            println!("    pubkey:    {}", p.pubkey);
        }
        EmbedKindProjection::Unknown(u) => {
            println!("✓ {label} → UnknownProjection (kind:{})", u.kind);
            println!("    content:   {}", truncate_for_display(&u.content, 80));
        }
    }
}

pub(crate) fn truncate_for_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}
