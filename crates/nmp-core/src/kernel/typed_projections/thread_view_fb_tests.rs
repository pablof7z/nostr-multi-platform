//! Round-trip proof for the `thread_view` Tier-2 typed codec.

use super::*;

fn timeline_item(idx: u64, populated: bool) -> TimelineItemModel {
    TimelineItemModel {
        id: format!("{idx:064}"),
        author_pubkey: "a".repeat(64),
        author_picture_url: populated.then(|| "https://img/a.png".to_string()),
        author_lnurl: populated.then(|| "a@wos.com".to_string()),
        author_display_name: populated.then(|| "Alice".to_string()),
        kind: 1,
        content: format!("reply {idx}"),
        content_preview: format!("reply {idx}"),
        created_at: 1_700_000_000 + idx,
        relay_count: 2,
        is_repost: false,
        nav_target_id: format!("{idx:064}"),
        repost_inner_content: String::new(),
    }
}

fn sample() -> ThreadViewModel {
    ThreadViewModel {
        focused_event_id: "f".repeat(64),
        root_event_id: "r".repeat(64),
        state: "ready".to_string(),
        items: vec![timeline_item(0, true), timeline_item(1, false)],
        previous_count: 1,
        next_count: 3,
        previous_count_label: "Show 1 earlier".to_string(),
        next_count_label: "3 more replies".to_string(),
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let bytes = encode_thread_view(&model);
    let decoded = decode_thread_view(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
}

#[test]
fn empty_items_and_zero_counts_round_trip() {
    let model = ThreadViewModel {
        focused_event_id: "f".repeat(64),
        root_event_id: "f".repeat(64),
        state: "opening".to_string(),
        items: Vec::new(),
        previous_count: 0,
        next_count: 0,
        previous_count_label: String::new(),
        next_count_label: String::new(),
    };
    let bytes = encode_thread_view(&model);
    let decoded = decode_thread_view(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model);
    assert!(decoded.items.is_empty());
}

#[test]
fn timeline_item_options_survive_distinctly() {
    let decoded = decode_thread_view(&encode_thread_view(&sample())).expect("decode");
    assert!(decoded.items[0].author_display_name.is_some());
    assert!(decoded.items[1].author_display_name.is_none());
}

#[test]
fn buffer_carries_the_ktvw_file_identifier() {
    let bytes = encode_thread_view(&sample());
    assert_eq!(
        &bytes[4..8],
        THREAD_VIEW_FILE_IDENTIFIER,
        "the buffer must embed the KTVW file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_thread_view(&[]).is_err());
    assert!(decode_thread_view(b"NMPU0000").is_err());
}
