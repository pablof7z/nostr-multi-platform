use super::*;
use crate::app::dynamic_feeds::DynamicFeedRuntime;
use crate::timeline::TimelineRow;
use std::cell::RefCell;

#[derive(Default)]
struct RecordingRuntime {
    calls: RefCell<Vec<String>>,
}

impl DynamicFeedRuntime for RecordingRuntime {
    fn open_author(&self, pubkey: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("open_author:{pubkey}"));
        Ok(())
    }

    fn close_author(&self, pubkey: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("close_author:{pubkey}"));
        Ok(())
    }

    fn open_thread(&self, event_id: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("open_thread:{event_id}"));
        Ok(())
    }

    fn close_thread(&self, event_id: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("close_thread:{event_id}"));
        Ok(())
    }

    fn resolve_open_profile(&self, pubkey: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("resolve_open_profile:{pubkey}"));
        Ok(())
    }

    fn release_open_profile(&self, pubkey: &str) -> crate::Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("release_open_profile:{pubkey}"));
        Ok(())
    }
}

fn row(id: &str, author: &str) -> TimelineRow {
    let snapshot = serde_json::json!({
        "cards": [{
            "card": {
                "id": id,
                "author_pubkey": author,
                "kind": 1,
                "created_at": 1_700_000_000_u64,
                "content": "row",
                "content_tree": { "nodes": [], "roots": [], "mode": "Plain" },
                "relation_counts": {}
            },
            "attribution": []
        }]
    });
    TimelineRow::from_snapshot(&snapshot)
        .into_iter()
        .next()
        .expect("row decodes")
}

#[test]
fn esc_closes_visible_author_feed_and_removes_rows() {
    let runtime = RecordingRuntime::default();
    let author = "55".repeat(32);
    let mut state = AppState {
        focused: Pane::Profile,
        profile_pubkey: author.clone(),
        profile_rows: vec![row("profile-row", &author)],
        ..Default::default()
    };

    super::feed_navigation::handle_escape(&mut state, &runtime);

    assert_eq!(
        runtime.calls.borrow().as_slice(),
        &[
            format!("close_author:{author}"),
            // ADR-0063 (#1671 Lane F): closing the pane releases its
            // profile.card/Live ref (D5 — bounded by the open view).
            format!("release_open_profile:{author}"),
        ],
        "Esc on a visible profile pane must dispatch the kernel author-feed close"
    );
    assert!(state.profile_pubkey.is_empty());
    assert!(state.profile_rows.is_empty());
    assert_eq!(state.focused, Pane::Feed);
    assert_eq!(state.status, "closed profile feed");
}
