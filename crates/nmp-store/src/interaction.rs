//! Shared interaction-counter classifier.
//!
//! Determines, from an event's kind and tags alone, whether it is an
//! interaction with another event and which category it falls into.
//!
//! Used by both the LMDB backend (`lmdb/interaction_counters.rs`) and the
//! in-memory backend (`mem/insert.rs`) so the classification logic is a
//! single source of truth — no duplication, no drift.

/// Which kind of social interaction an event represents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum CounterKind {
    Reply = 1,
    Reaction = 2,
    Repost = 3,
    Zap = 4,
}

/// Classify an event by its `kind` and `tags` into an interaction type and
/// the target event-id hex string, or `None` if the event is not an
/// interaction counter.
///
/// ## Reply marker precedence (kind:1)
///
/// For kind:1 events NIP-10 defines three roles for e-tags: "root", "reply",
/// and "mention". We target the first e-tag that carries the "reply" marker,
/// then the first with "root", then the first e-tag with no marker (legacy).
///
/// ## Other kinds
///
/// kind:7 (Reaction), kind:6 (Repost), kind:9735 (Zap) — target is the first
/// e-tag value regardless of marker.
pub(crate) fn classify(kind: u32, tags: &[Vec<String>]) -> Option<(CounterKind, String)> {
    match kind {
        1 => classify_reply(tags),
        7 => first_e_tag(tags).map(|id| (CounterKind::Reaction, id)),
        6 => first_e_tag(tags).map(|id| (CounterKind::Repost, id)),
        9735 => first_e_tag(tags).map(|id| (CounterKind::Zap, id)),
        _ => None,
    }
}

/// Reply marker precedence: "reply" > "root" > first bare e-tag.
fn classify_reply(tags: &[Vec<String>]) -> Option<(CounterKind, String)> {
    let mut reply_id: Option<String> = None;
    let mut root_id: Option<String> = None;
    let mut first_id: Option<String> = None;

    for tag in tags {
        if tag.len() < 2 || tag[0] != "e" {
            continue;
        }
        let id = tag[1].clone();
        let marker = tag.get(3).map(|s| s.as_str()).unwrap_or("");
        if marker == "reply" && reply_id.is_none() {
            reply_id = Some(id);
        } else if marker == "root" && root_id.is_none() {
            root_id = Some(id);
        } else if first_id.is_none() && marker != "reply" && marker != "root" {
            first_id = Some(id);
        }
    }
    let target = reply_id.or(root_id).or(first_id)?;
    Some((CounterKind::Reply, target))
}

/// Return the first e-tag value, or `None` if there are none.
fn first_e_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter()
        .find(|tag| tag.len() >= 2 && tag[0] == "e")
        .map(|tag| tag[1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn etag(id: &str) -> Vec<String> {
        vec!["e".into(), id.into()]
    }
    fn etag_m(id: &str, marker: &str) -> Vec<String> {
        vec!["e".into(), id.into(), "wss://r/".into(), marker.into()]
    }
    fn ptag(pk: &str) -> Vec<String> {
        vec!["p".into(), pk.into()]
    }

    #[test]
    fn kind1_no_etag_returns_none() {
        assert!(classify(1, &[ptag("aabb")]).is_none());
    }

    #[test]
    fn kind1_bare_etag_is_reply() {
        let tags = vec![etag("deadbeef")];
        let r = classify(1, &tags).unwrap();
        assert_eq!(r.0, CounterKind::Reply);
        assert_eq!(r.1, "deadbeef");
    }

    #[test]
    fn kind1_reply_marker_wins_over_root() {
        let tags = vec![etag_m("aaa", "root"), etag_m("bbb", "reply")];
        let r = classify(1, &tags).unwrap();
        assert_eq!(r.1, "bbb");
    }

    #[test]
    fn kind1_root_marker_wins_over_bare() {
        let tags = vec![etag("bare"), etag_m("root_id", "root")];
        let r = classify(1, &tags).unwrap();
        assert_eq!(r.1, "root_id");
    }

    #[test]
    fn kind7_first_etag() {
        let tags = vec![etag("target7")];
        let r = classify(7, &tags).unwrap();
        assert_eq!(r.0, CounterKind::Reaction);
        assert_eq!(r.1, "target7");
    }

    #[test]
    fn kind6_first_etag() {
        let tags = vec![etag("target6")];
        let r = classify(6, &tags).unwrap();
        assert_eq!(r.0, CounterKind::Repost);
    }

    #[test]
    fn kind9735_first_etag() {
        let tags = vec![etag("zapzap")];
        let r = classify(9735, &tags).unwrap();
        assert_eq!(r.0, CounterKind::Zap);
    }

    #[test]
    fn non_interaction_kind_returns_none() {
        assert!(classify(0, &[etag("x")]).is_none());
        assert!(classify(3, &[etag("x")]).is_none());
        assert!(classify(30023, &[etag("x")]).is_none());
    }
}
