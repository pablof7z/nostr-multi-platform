//! Timeline ordering regressions for the incremental visible-list path.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

fn seed_event(kernel: &mut Kernel, id: &str, created_at: u64) {
    kernel.events.insert(
        id.to_string(),
        StoredEvent {
            id: id.to_string(),
            author: "author".to_string(),
            kind: 1,
            created_at,
            tags: Vec::new(),
            content: String::new(),
            relay_count: 1,
        },
    );
}

#[test]
fn incremental_timeline_insert_matches_sort_order_and_cap() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    for i in 0..510 {
        let id = format!("{i:064x}");
        let created_at = 1_700_000_000 + ((i * 37) % 510) as u64;
        seed_event(&mut kernel, &id, created_at);
        kernel.insert_timeline_id_sorted(id);
    }

    assert_eq!(kernel.timeline.len(), 500);
    assert!(kernel
        .timeline
        .iter()
        .zip(kernel.timeline.iter().skip(1))
        .all(|(left, right)| {
            let a = kernel.events.get(left).expect("left event is cached");
            let b = kernel.events.get(right).expect("right event is cached");
            b.created_at < a.created_at || (b.created_at == a.created_at && left <= right)
        }));
}
