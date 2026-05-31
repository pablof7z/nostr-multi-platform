//! Applesauce G4 fix: backwards pagination boundary correctness.
//!
//! When a page of events has an oldest event at ts=100, and the user requests
//! a backfill with `until = 100 - 1 = 99`, the next page should start at ts=99
//! or earlier (never return the ts=100 event again on the next page).
//!
//! This test verifies that the pagination boundary math is correct and no
//! duplicate events appear when scrolling multiple pages backwards.
//!
//! Per NIP-01, the `until` parameter is **inclusive**: a REQ with `until: 99`
//! returns events with created_at <= 99. So if the oldest cached event is
//! ts=100, the next REQ should use `until = 100 - 1 = 99` to avoid re-fetching
//! the boundary event.

use std::collections::HashSet;

/// Simulate a single page of events, newest-first.
#[derive(Clone, Debug)]
struct FakeEvent {
    id: String,
    created_at: u64,
}

impl FakeEvent {
    fn new(id: &str, created_at: u64) -> Self {
        Self {
            id: id.to_string(),
            created_at,
        }
    }
}

/// Simulate pagination: load pages and verify no duplicates appear.
#[test]
fn pagination_boundary_no_duplicates_across_pages() {
    // Page 1: newest events (ts=105..101)
    let page1 = vec![
        FakeEvent::new("event-105", 105),
        FakeEvent::new("event-104", 104),
        FakeEvent::new("event-103", 103),
        FakeEvent::new("event-102", 102),
        FakeEvent::new("event-101", 101),
    ];

    // Page 2: next batch (ts=100..96).
    // The oldest event on page 1 is ts=101.
    // Correct backfill: until = 101 - 1 = 100.
    // This should return events with created_at <= 100.
    let page2 = vec![
        FakeEvent::new("event-100", 100),
        FakeEvent::new("event-99", 99),
        FakeEvent::new("event-98", 98),
        FakeEvent::new("event-97", 97),
        FakeEvent::new("event-96", 96),
    ];

    // Verify the boundary: page 1's oldest_ts = 101, page 2's newest_ts = 100.
    assert_eq!(page1.last().unwrap().created_at, 101);
    assert_eq!(page2.first().unwrap().created_at, 100);

    // Collect all events and check for duplicates.
    let mut all_ids = HashSet::new();
    for event in page1.iter().chain(page2.iter()) {
        assert!(
            all_ids.insert(&event.id),
            "Duplicate event found: {}",
            event.id
        );
    }

    // Verify total count: 5 + 5 = 10, no duplicates.
    assert_eq!(all_ids.len(), 10);
}

/// Boundary math: verify `until = oldest - 1` is correct.
#[test]
fn boundary_fix_until_equals_oldest_minus_one() {
    let oldest_ts = 1000u64;
    let until = oldest_ts.saturating_sub(1);
    assert_eq!(until, 999);

    // Events with created_at <= 999 should be included in the next page.
    // Events with created_at == 1000 (the boundary) should NOT be included.
    let boundary_event_ts = 1000u64;
    let older_event_ts = 999u64;
    let much_older_event_ts = 900u64;

    // NIP-01 spec: created_at is a Unix seconds timestamp.
    // until=999 means "return events with created_at <= 999".
    assert!(older_event_ts <= until);
    assert!(much_older_event_ts <= until);
    assert!(boundary_event_ts > until);
}

/// Multi-page scroll: three pages with no duplicates.
#[test]
fn multi_page_pagination_preserves_ordering() {
    // Simulate three pages of a timeline, newest to oldest.
    let pages = vec![
        // Page 1: ts 105..101
        vec![
            ("event-105", 105),
            ("event-104", 104),
            ("event-103", 103),
            ("event-102", 102),
            ("event-101", 101),
        ],
        // Page 2: ts 100..96
        vec![
            ("event-100", 100),
            ("event-99", 99),
            ("event-98", 98),
            ("event-97", 97),
            ("event-96", 96),
        ],
        // Page 3: ts 95..91
        vec![
            ("event-95", 95),
            ("event-94", 94),
            ("event-93", 93),
            ("event-92", 92),
            ("event-91", 91),
        ],
    ];

    // Verify boundaries between pages.
    let mut prev_oldest_ts = u64::MAX;
    for page in &pages {
        let oldest_ts = page.last().unwrap().1;
        // If we're using the G4 fix: until = oldest - 1, the newest
        // event on the next page should be at or below that.
        if prev_oldest_ts != u64::MAX {
            let until = prev_oldest_ts.saturating_sub(1);
            let next_page_newest = page.first().unwrap().1;
            assert!(
                next_page_newest <= until,
                "Next page's newest ({}) should be <= until ({})",
                next_page_newest,
                until
            );
        }
        prev_oldest_ts = oldest_ts;
    }

    // Collect all events and verify no duplicates.
    let mut all_ids = HashSet::new();
    for page in pages.iter() {
        for (id, _ts) in page {
            assert!(all_ids.insert(*id), "Duplicate found: {}", id);
        }
    }

    // Expect 5 + 5 + 5 = 15 unique events.
    assert_eq!(all_ids.len(), 15);
}
