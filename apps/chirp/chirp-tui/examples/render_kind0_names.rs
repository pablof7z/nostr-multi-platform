use chirp_tui::timeline::TimelineRow;
use chirp_tui::ui::colors::{author_color, BODY_TEXT, DIM_TEXT};
use chirp_tui::ui::nostr_content::nostr_content_view::NostrContentView;
use chirp_tui::ui::nostr_user::profile_name_span;
use nmp_core::nip19::decode_npub;
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_nip01::{ModularTimelineProjection, ModularTimelineSpec};
use ratatui::backend::TestBackend;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Terminal;
use serde_json::{json, Value};

const NOTE_JSON: &str = r#"{"kind":1,"id":"7b427e3e641069b554148eeaabd0d6413acead8c4e2c72b07b3d4240974846e1","pubkey":"3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d","created_at":1779301972,"tags":[["q","784bb31500f6ac1515860037f9025e7ef33d0953256ea78ea0c97e0ef6bdb9f5","wss://relay.ditto.pub/"]],"content":"I've used 'nak key combine <pubkey1> <pubkey2>' to confirm that nostr:npub1vagelp6lznaz3me6w5atqvy5ema4fe3w98ch8atdff22p672gtmqlsq7dm is really the merge of nostr:npub1t6jxfqz9hv0lygn9thwndekuahwyxkgvycyscjrtauuw73gd5k7sqvksrw and nostr:npub1ryqlnhsd3wte8hk3euxmxrdga48gx7x6y50fz3descyavl98azmq0f49ws and you should do that too!\n\nnostr:nevent1qvzqqqqqqypzqe63n7r479869rhn5af6kqcffnhm2nnzu203w06k6jj55r4u5shkqyt8wumn8ghj7un9d3shjtnyd968gmewwp6kytcqypuyhvc4qrm2c9g4scqr07gztel0x0gf2vjkafuw5ryhurhkhkul263za78","sig":"183edc04ac5e86250d8d83a7e17e33ddd3708e4dba9a43dad0279a208d243961a9f6e4c625624e5e9ed4c8d99b28ca3ae49b30c8be7ce6b5649905292139deb2"}"#;

const MERGED_NPUB: &str = "npub1vagelp6lznaz3me6w5atqvy5ema4fe3w98ch8atdff22p672gtmqlsq7dm";
const CONSTANT_NPUB: &str = "npub1t6jxfqz9hv0lygn9thwndekuahwyxkgvycyscjrtauuw73gd5k7sqvksrw";
const HIGH_TEMPLAR_NPUB: &str = "npub1ryqlnhsd3wte8hk3euxmxrdga48gx7x6y50fz3descyavl98azmq0f49ws";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let projection = ModularTimelineProjection::new(&ModularTimelineSpec {
        viewer: "chirp-tui-render-harness".to_string(),
        kinds: vec![1],
        authors: None,
        policy: Default::default(),
    });
    projection.on_kernel_event(&note_event()?);

    let before = render_projection(&projection)?;
    println!("render #1: before kind:0 profiles");
    println!("{before}");

    projection.on_kernel_event(&profile_event(
        "author-profile",
        "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
        "Sparrow",
    ));
    projection.on_kernel_event(&profile_event(
        "merged-profile",
        &decode_npub(MERGED_NPUB)?,
        "(Constant-Archon)",
    ));
    projection.on_kernel_event(&profile_event(
        "constant-profile",
        &decode_npub(CONSTANT_NPUB)?,
        "Constant",
    ));
    projection.on_kernel_event(&profile_event(
        "templar-profile",
        &decode_npub(HIGH_TEMPLAR_NPUB)?,
        "High Templar",
    ));

    let after = render_projection(&projection)?;
    println!("render #2: after kind:0 profiles");
    println!("{after}");

    assert!(
        before.contains("@npub1v"),
        "first render should show npub fallback"
    );
    assert!(
        after.contains("@(Constant-Archon)")
            && after.contains("@Constant")
            && after.contains("@High Templar"),
        "second render should resolve mention names:\n{after}"
    );
    assert!(
        after.contains("that @(Constant-Archon) is really")
            && after.contains("merge of @Constant")
            && after.contains("and @High Templar"),
        "resolved mentions should stay inline in the rendered note:\n{after}"
    );
    assert!(
        after.contains("Sparrow"),
        "second render should resolve the note author name:\n{after}"
    );

    Ok(())
}

fn note_event() -> Result<KernelEvent, serde_json::Error> {
    let value: Value = serde_json::from_str(NOTE_JSON)?;
    Ok(KernelEvent {
        id: string(&value, "id"),
        author: string(&value, "pubkey"),
        kind: value.get("kind").and_then(Value::as_u64).unwrap_or(1) as u32,
        created_at: value.get("created_at").and_then(Value::as_u64).unwrap_or(0),
        tags: value
            .get("tags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|tag| {
                tag.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .collect(),
        content: string(&value, "content"),
        relay_provenance: Vec::new(),
    })
}

fn profile_event(id: &str, pubkey: &str, display_name: &str) -> KernelEvent {
    KernelEvent {
        id: format!("{id:0<64}").chars().take(64).collect(),
        author: pubkey.to_string(),
        kind: 0,
        created_at: 1_779_301_973,
        tags: Vec::new(),
        content: json!({ "display_name": display_name }).to_string(),
        relay_provenance: Vec::new(),
    }
}

fn render_projection(
    projection: &ModularTimelineProjection,
) -> Result<String, Box<dyn std::error::Error>> {
    let snapshot = serde_json::to_value(projection.snapshot())?;
    let rows = TimelineRow::from_snapshot(&snapshot);
    let row = rows
        .first()
        .ok_or_else(|| "projection did not produce a timeline row".to_string())?;
    render_row(row)
}

fn render_row(row: &TimelineRow) -> Result<String, Box<dyn std::error::Error>> {
    let mut lines = Vec::new();
    let author_style = Style::default()
        .fg(author_color(&row.author_pubkey))
        .add_modifier(Modifier::BOLD);
    let (author, _) = profile_name_span(&row.author_profile, author_style, 80);
    lines.push(Line::from(vec![
        author,
        Span::styled(" · author", Style::default().fg(DIM_TEXT)),
    ]));
    lines.extend(
        row.content_tree
            .as_ref()
            .map(|tree| {
                NostrContentView::new(tree)
                    .render_data(Some(&row.content_render))
                    .lines(118)
            })
            .unwrap_or_else(|| vec![Line::from(row.content.clone())]),
    );

    let backend = TestBackend::new(120, 10);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| {
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().fg(BODY_TEXT))
                .wrap(Wrap { trim: false }),
            frame.area(),
        );
    })?;
    Ok(buffer_text(terminal.backend().buffer()))
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = *buffer.area();
    let mut lines = Vec::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        for x in area.x..area.x + area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n").trim_end().to_string()
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
