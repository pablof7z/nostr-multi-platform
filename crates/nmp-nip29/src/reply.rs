//! NIP-10 reply / thread edge extraction for h-tagged group events.
//!
//! NIP-29 owns only the `["h", local_id]` routing concern; threading on a group
//! event (kind:9 reply, kind:11 thread root/reply) is expressed with the
//! **NIP-10** `e`-tag marker grammar, which this module parses into a flat
//! `(root, reply_to)` edge pair for a read-side projection to surface.
//!
//! This parser is **kind-agnostic** (it inspects only `e` tags) and ships raw
//! data only (aim.md §2): the two outputs are raw hex event ids, never display
//! strings.
//!
//! ## The NIP-10 `e`-tag grammar
//!
//! A marked `e` tag is `["e", <event-id>, <relay-url>, <marker>, <pubkey?>]`
//! where `<marker>` ∈ `{"root", "reply", "mention"}` (index 3). The preferred
//! (marked) form is unambiguous:
//!
//! - `root`  — the thread root the event belongs to.
//! - `reply` — the immediate parent the event replies to.
//! - `mention` — a quote / reference; **never** a thread edge (ignored here).
//!
//! A top-level reply to the thread root carries only a `root` marker (no
//! `reply`); a nested reply carries both. So `reply_to` falls back to the
//! `root` marker when no explicit `reply` marker is present, and `root` falls
//! back to the `reply` marker when a (malformed) tag set marks only a reply.
//!
//! ## Deprecated positional form
//!
//! When NO `e` tag carries a marker, NIP-10's deprecated positional convention
//! applies over the marker-less `e` tags (mentions cannot be distinguished, so
//! every marker-less `e` tag participates):
//!
//! - 1 tag  → that event is both the root and the parent (`root == reply_to`).
//! - 2+ tags → first is the root, last is the immediate parent.

/// A flat reply/thread edge pair extracted from an event's `e` tags.
///
/// Both fields are raw hex event ids or `None` when the event is a thread root
/// (or carries no `e` tags at all). `root == reply_to` for a direct reply to a
/// thread root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplyEdges {
    /// The thread root event id, if this event is part of a thread.
    pub root: Option<String>,
    /// The immediate parent event id this event replies to, if any.
    pub reply_to: Option<String>,
}

impl ReplyEdges {
    /// `true` when neither a root nor a reply edge was found — the event is a
    /// standalone post / thread root.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.root.is_none() && self.reply_to.is_none()
    }
}

/// NIP-10 `e`-tag marker (index 3 of a marked `e` tag).
const MARKER_ROOT: &str = "root";
const MARKER_REPLY: &str = "reply";
const MARKER_MENTION: &str = "mention";

/// Parse the NIP-10 reply / thread edges from an event's `tags`.
///
/// Prefers the marked grammar; falls back to the deprecated positional
/// convention only when no `e` tag is marked. `mention`-marked tags never
/// contribute a thread edge. A tag with an empty event id is ignored.
#[must_use]
pub fn parse_reply_edges(tags: &[Vec<String>]) -> ReplyEdges {
    let mut marked_root: Option<&str> = None;
    let mut marked_reply: Option<&str> = None;
    let mut any_marker = false;
    let mut positional: Vec<&str> = Vec::new();

    for tag in tags {
        if tag.first().map(String::as_str) != Some("e") {
            continue;
        }
        let Some(event_id) = tag.get(1).map(String::as_str).filter(|id| !id.is_empty()) else {
            continue;
        };
        match tag.get(3).map(String::as_str) {
            Some(MARKER_ROOT) => {
                any_marker = true;
                if marked_root.is_none() {
                    marked_root = Some(event_id);
                }
            }
            Some(MARKER_REPLY) => {
                any_marker = true;
                if marked_reply.is_none() {
                    marked_reply = Some(event_id);
                }
            }
            Some(MARKER_MENTION) => {
                // A quote / reference — observed as a marker (so positional
                // fallback stays disabled) but never a thread edge.
                any_marker = true;
            }
            // An unknown or absent marker. Empty-string markers
            // (`["e", id, relay, ""]`) are treated as positional too.
            _ => positional.push(event_id),
        }
    }

    if any_marker {
        // Marked grammar. `reply_to` is the immediate parent (the `reply`
        // marker), falling back to `root` for a top-level reply; `root` is the
        // thread root, falling back to the `reply` marker for a tag set that
        // (incorrectly) marks only a reply.
        return ReplyEdges {
            root: marked_root.or(marked_reply).map(str::to_string),
            reply_to: marked_reply.or(marked_root).map(str::to_string),
        };
    }

    // Deprecated positional convention over the marker-less `e` tags.
    match positional.as_slice() {
        [] => ReplyEdges::default(),
        [single] => ReplyEdges {
            root: Some((*single).to_string()),
            reply_to: Some((*single).to_string()),
        },
        [first, .., last] => ReplyEdges {
            root: Some((*first).to_string()),
            reply_to: Some((*last).to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, marker: Option<&str>) -> Vec<String> {
        match marker {
            Some(m) => vec!["e".into(), id.into(), String::new(), m.into()],
            None => vec!["e".into(), id.into()],
        }
    }

    #[test]
    fn no_e_tags_is_empty() {
        let edges = parse_reply_edges(&[vec!["h".into(), "room".into()]]);
        assert!(edges.is_empty());
        assert_eq!(edges, ReplyEdges::default());
    }

    #[test]
    fn marked_root_only_is_top_level_reply() {
        // A direct reply to the thread root: only a `root` marker. The parent
        // IS the root.
        let edges = parse_reply_edges(&[e("root1", Some("root"))]);
        assert_eq!(edges.root.as_deref(), Some("root1"));
        assert_eq!(edges.reply_to.as_deref(), Some("root1"));
    }

    #[test]
    fn marked_root_and_reply_is_nested_reply() {
        let edges = parse_reply_edges(&[
            e("root1", Some("root")),
            e("parent2", Some("reply")),
            e("mentioned3", Some("mention")),
        ]);
        assert_eq!(edges.root.as_deref(), Some("root1"));
        assert_eq!(edges.reply_to.as_deref(), Some("parent2"));
    }

    #[test]
    fn mention_marker_is_never_a_thread_edge() {
        let edges = parse_reply_edges(&[e("quoted1", Some("mention"))]);
        assert!(edges.is_empty());
    }

    #[test]
    fn reply_marker_only_falls_back_to_reply_for_root() {
        let edges = parse_reply_edges(&[e("parent2", Some("reply"))]);
        assert_eq!(edges.root.as_deref(), Some("parent2"));
        assert_eq!(edges.reply_to.as_deref(), Some("parent2"));
    }

    #[test]
    fn positional_single_is_root_and_parent() {
        let edges = parse_reply_edges(&[e("only1", None)]);
        assert_eq!(edges.root.as_deref(), Some("only1"));
        assert_eq!(edges.reply_to.as_deref(), Some("only1"));
    }

    #[test]
    fn positional_two_is_first_root_last_parent() {
        let edges = parse_reply_edges(&[e("root1", None), e("parent2", None)]);
        assert_eq!(edges.root.as_deref(), Some("root1"));
        assert_eq!(edges.reply_to.as_deref(), Some("parent2"));
    }

    #[test]
    fn positional_three_uses_first_and_last() {
        let edges = parse_reply_edges(&[e("root1", None), e("mid2", None), e("parent3", None)]);
        assert_eq!(edges.root.as_deref(), Some("root1"));
        assert_eq!(edges.reply_to.as_deref(), Some("parent3"));
    }

    #[test]
    fn empty_event_id_is_ignored() {
        let edges = parse_reply_edges(&[e("", None), e("real1", None)]);
        assert_eq!(edges.root.as_deref(), Some("real1"));
        assert_eq!(edges.reply_to.as_deref(), Some("real1"));
    }

    #[test]
    fn first_marked_root_wins_over_later_duplicates() {
        let edges = parse_reply_edges(&[e("root1", Some("root")), e("root2", Some("root"))]);
        assert_eq!(edges.root.as_deref(), Some("root1"));
    }
}
