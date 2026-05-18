//! `ViewModule` implementation for the podcast library.
//!
//! [`LibraryViewModule`] projects the set of subscribed [`PodcastRecord`]
//! rows (written by the action layer into the `"podcast.podcasts"` domain
//! namespace) into a [`LibraryView`] payload for the UI.
//!
//! Design contract:
//!
//! * **Spec** — `LibrarySpec {}` (singleton view — one per app instance).
//! * **Key** — `()` (only one library view can be open at a time).
//! * **State** — `LibraryState`, a `Vec<PodcastRecord>` maintained by
//!   `on_projection_changed` callbacks.
//! * **Payload** — [`LibraryView`] with one [`PodcastRowPayload`] per record.
//! * **Delta** — `LibraryView` (full replacement on every change — the library
//!   is small enough that incremental deltas add no meaningful value today).
//!
//! This is a **pure state-machine**: no I/O, no async, no side-effects.
//! Tests drive it by constructing `LibraryState` directly and calling the
//! trait methods.
//!
//! D0: no podcast nouns in `nmp-core`; this file lives under
//! `apps/podcast/podcast-core`.

use serde::{Deserialize, Serialize};

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};

use crate::domain::records::PodcastRecord;
use crate::views::{LibraryView, PodcastRowPayload};

// ─── Spec ─────────────────────────────────────────────────────────────────────

/// Singleton spec — no parameters. Only one library view per session.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct LibrarySpec {}

// ─── State ────────────────────────────────────────────────────────────────────

/// Internal mutable state for the library view.
///
/// Maintained by `on_projection_changed`; read by `snapshot()`.
#[derive(Default)]
pub struct LibraryState {
    pub records: Vec<PodcastRecord>,
}

// ─── ViewModule impl ──────────────────────────────────────────────────────────

/// View module that projects `PodcastRecord` rows into a [`LibraryView`].
///
/// Namespace: `"podcast.library"`.
pub struct LibraryViewModule;

impl ViewModule for LibraryViewModule {
    const NAMESPACE: &'static str = "podcast.library";

    type Spec = LibrarySpec;
    type Payload = LibraryView;
    type Delta = LibraryView;
    type Key = ();
    type State = LibraryState;

    fn key(_spec: &Self::Spec) -> Self::Key {}

    fn dependencies(_spec: &Self::Spec) -> ViewDependencies {
        // Pure domain-store view — no Nostr event subscriptions.
        ViewDependencies::default()
    }

    fn open(_ctx: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        let state = LibraryState::default();
        let payload = LibraryView::default();
        (state, payload)
    }

    fn on_event_inserted(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _event: &KernelEvent,
    ) -> Option<Self::Delta> {
        None // No Nostr event ingest for the library view.
    }

    fn on_event_removed(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _id: &EventId,
    ) -> Option<Self::Delta> {
        None
    }

    fn on_event_replaced(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _old_id: &EventId,
        _new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        None
    }

    /// Receives a projection change from the action layer.
    ///
    /// Payload shape (JSON):
    /// - `{"op":"insert","record":{...PodcastRecord...}}` — add/replace record.
    /// - `{"op":"remove","id":"<ULID>"}` — remove by podcast id.
    ///
    /// Unknown shapes are silently ignored (D6).
    fn on_projection_changed(
        _ctx: &ViewContext,
        state: &mut Self::State,
        change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        if change.namespace != Self::NAMESPACE {
            return None;
        }
        let op = change.payload.get("op").and_then(|v| v.as_str())?;
        match op {
            "insert" => {
                let record: PodcastRecord =
                    serde_json::from_value(change.payload["record"].clone()).ok()?;
                let id = record.id;
                if let Some(pos) = state.records.iter().position(|r| r.id == id) {
                    state.records[pos] = record;
                } else {
                    state.records.push(record);
                }
                Some(Self::make_payload(state))
            }
            "remove" => {
                let id_str = change.payload["id"].as_str()?;
                let id: ulid::Ulid = id_str.parse().ok()?;
                let before = state.records.len();
                state.records.retain(|r| r.id != id);
                if state.records.len() != before {
                    Some(Self::make_payload(state))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn snapshot(_ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        Self::make_payload(state)
    }
}

impl LibraryViewModule {
    fn make_payload(state: &LibraryState) -> LibraryView {
        let podcasts = state
            .records
            .iter()
            .map(|r| PodcastRowPayload {
                id: r.id.to_string(),
                title: r.title.clone(),
                author: r.author.clone(),
                artwork_url: r.artwork_url.as_ref().map(|u| u.to_string()),
                episode_count: 0, // Episode count updated by EpisodesModule projection changes.
            })
            .collect();
        LibraryView { podcasts }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{ModuleFamily, ModuleRegistry};

    fn ctx() -> ViewContext {
        ViewContext::default()
    }

    fn make_record(title: &str, feed_url: &str) -> PodcastRecord {
        PodcastRecord {
            id: ulid::Ulid::new(),
            feed_url: feed_url.parse().unwrap(),
            title: title.to_owned(),
            author: String::new(),
            artwork_url: None,
            subscribed_at_ms: 0,
            last_refreshed_ms: None,
        }
    }

    fn insert_change(record: &PodcastRecord) -> ProjectionChange {
        ProjectionChange {
            namespace: "podcast.library".to_owned(),
            key: record.id.to_string(),
            payload: serde_json::json!({
                "op": "insert",
                "record": serde_json::to_value(record).unwrap(),
            }),
        }
    }

    fn remove_change(id: &ulid::Ulid) -> ProjectionChange {
        ProjectionChange {
            namespace: "podcast.library".to_owned(),
            key: id.to_string(),
            payload: serde_json::json!({
                "op": "remove",
                "id": id.to_string(),
            }),
        }
    }

    // ─── ViewModule state-machine tests ───────────────────────────────────────

    #[test]
    fn open_yields_empty_library() {
        let (_state, payload) = LibraryViewModule::open(&ctx(), LibrarySpec::default());
        assert!(payload.podcasts.is_empty());
    }

    #[test]
    fn insert_change_adds_podcast_row() {
        let (_init, _) = LibraryViewModule::open(&ctx(), LibrarySpec::default());
        let mut state = LibraryState::default();
        let rec = make_record("My Show", "https://feeds.example.com/show.xml");
        let delta = LibraryViewModule::on_projection_changed(&ctx(), &mut state, &insert_change(&rec));
        let delta = delta.expect("insert change must produce delta");
        assert_eq!(delta.podcasts.len(), 1);
        assert_eq!(delta.podcasts[0].title, "My Show");
    }

    #[test]
    fn snapshot_reflects_inserted_rows() {
        let mut state = LibraryState::default();
        let rec = make_record("Show A", "https://a.example.com/feed.xml");
        LibraryViewModule::on_projection_changed(&ctx(), &mut state, &insert_change(&rec));
        let snap = LibraryViewModule::snapshot(&ctx(), &state);
        assert_eq!(snap.podcasts.len(), 1);
        assert_eq!(snap.podcasts[0].title, "Show A");
    }

    #[test]
    fn remove_change_removes_podcast_row() {
        let mut state = LibraryState::default();
        let rec = make_record("Show B", "https://b.example.com/feed.xml");
        LibraryViewModule::on_projection_changed(&ctx(), &mut state, &insert_change(&rec));
        let delta = LibraryViewModule::on_projection_changed(
            &ctx(),
            &mut state,
            &remove_change(&rec.id),
        );
        let delta = delta.expect("remove change must produce delta");
        assert!(delta.podcasts.is_empty());
        assert!(LibraryViewModule::snapshot(&ctx(), &state).podcasts.is_empty());
    }

    #[test]
    fn remove_unknown_id_produces_no_delta() {
        let mut state = LibraryState::default();
        let unknown = ulid::Ulid::new();
        let delta =
            LibraryViewModule::on_projection_changed(&ctx(), &mut state, &remove_change(&unknown));
        assert!(delta.is_none(), "removing unknown id must produce no delta");
    }

    #[test]
    fn insert_same_id_replaces_record() {
        let mut state = LibraryState::default();
        let mut rec = make_record("Old Title", "https://c.example.com/feed.xml");
        LibraryViewModule::on_projection_changed(&ctx(), &mut state, &insert_change(&rec));
        rec.title = "New Title".to_owned();
        let delta =
            LibraryViewModule::on_projection_changed(&ctx(), &mut state, &insert_change(&rec));
        let delta = delta.expect("upsert must produce delta");
        assert_eq!(delta.podcasts.len(), 1);
        assert_eq!(delta.podcasts[0].title, "New Title");
    }

    #[test]
    fn nostr_events_produce_no_delta() {
        let mut state = LibraryState::default();
        let event = KernelEvent {
            id: "aaaa".into(),
            author: "bbbb".into(),
            kind: 1,
            created_at: 0,
            tags: vec![],
            content: "hello".into(),
        };
        assert!(
            LibraryViewModule::on_event_inserted(&ctx(), &mut state, &event).is_none(),
            "library view must not react to Nostr events"
        );
    }

    #[test]
    fn wrong_namespace_change_is_ignored() {
        let mut state = LibraryState::default();
        let change = ProjectionChange {
            namespace: "other.namespace".to_owned(),
            key: "x".into(),
            payload: serde_json::json!({"op": "insert", "record": {}}),
        };
        assert!(
            LibraryViewModule::on_projection_changed(&ctx(), &mut state, &change).is_none(),
            "wrong namespace must be ignored"
        );
    }

    // ─── ModuleRegistry integration ───────────────────────────────────────────

    #[test]
    fn library_view_module_registers_in_module_registry() {
        let mut reg = ModuleRegistry::default();
        reg.register_view::<LibraryViewModule>();
        let descriptors = reg.descriptors();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].namespace, "podcast.library");
        assert_eq!(descriptors[0].family, ModuleFamily::View);
    }
}
