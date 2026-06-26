//! Interaction aggregate counters (replies, reactions, reposts, zaps) for the
//! OPFS-SQLite engine (#1007 PR-5).
//!
//! ## Why on-read, not a denormalized counter table
//!
//! The LMDB backend keeps a denormalized `target → counts` sub-db that it
//! increments at insert / decrements at delete, because LMDB has no secondary
//! index to find "every kind:7 that e-tags X" cheaply. SQLite **does**
//! (`idx_tags_tci` on `event_tags(tag_name, tag_value, …)`), so the faithful
//! translation (Article VIII — trust the framework) computes the same counts
//! with one indexed query at read time: find the small set of events that e-tag
//! the target, run the shared [`classify`] on each, and tally the classified
//! target. The result is identical to the LMDB counter (which was itself built
//! by running `classify` at insert) — including NIP-10 reply-marker precedence,
//! which a flat `event_tags` count could not honour — without a counter table or
//! an insert-path hook.
//!
//! The classifier is pure and target-agnostic (a port of
//! `nmp-store/src/interaction.rs`), so it is unit-tested on native; the indexed
//! read is wasm-gated.

/// Which kind of social interaction an event represents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CounterKind {
    /// kind:1 reply (NIP-10 reply/root/bare `e`-tag).
    Reply,
    /// kind:7 reaction.
    Reaction,
    /// kind:6 repost.
    Repost,
    /// kind:9735 zap receipt.
    Zap,
}

/// Classify an event by `kind` + `tags` into an interaction type and the target
/// event-id hex, or `None` if it is not an interaction. NIP-10 reply precedence
/// for kind:1: first "reply"-marked `e`-tag, else first "root", else first bare.
pub(crate) fn classify(kind: u32, tags: &[Vec<String>]) -> Option<(CounterKind, String)> {
    match kind {
        1 => classify_reply(tags),
        7 => first_e_tag(tags).map(|id| (CounterKind::Reaction, id)),
        6 => first_e_tag(tags).map(|id| (CounterKind::Repost, id)),
        9735 => first_e_tag(tags).map(|id| (CounterKind::Zap, id)),
        _ => None,
    }
}

fn classify_reply(tags: &[Vec<String>]) -> Option<(CounterKind, String)> {
    let mut reply_id: Option<String> = None;
    let mut root_id: Option<String> = None;
    let mut first_id: Option<String> = None;
    for tag in tags {
        if tag.len() < 2 || tag[0] != "e" {
            continue;
        }
        let id = tag[1].clone();
        let marker = tag.get(3).map(String::as_str).unwrap_or("");
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

fn first_e_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter()
        .find(|tag| tag.len() >= 2 && tag[0] == "e")
        .map(|tag| tag[1].clone())
}

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use super::{classify, CounterKind};
    use crate::conv;
    use crate::error::SqliteWasmError;
    use crate::outcome::EventId;
    use crate::types::TargetInteractionCounts;
    use crate::OpfsSqliteStore;

    impl OpfsSqliteStore {
        /// Interaction counts for `target` — the number of stored reply /
        /// reaction / repost / zap events that reference it (by the classified
        /// target, so NIP-10 marker precedence is honoured). Index-served by
        /// `idx_tags_tci` on the candidate `e`-tag rows.
        pub fn interaction_counts(
            &self,
            target: &EventId,
        ) -> Result<TargetInteractionCounts, SqliteWasmError> {
            let target_hex = to_hex64(target);
            let conn = self.db.borrow();
            // Candidates: events of an interaction kind that carry an `e`-tag
            // pointing at the target. Decode each and tally its CLASSIFIED
            // target (a kind:1 with both root+reply markers counts only once).
            let stmt = conn.prepare(
                "SELECT e.kind, e.raw FROM event_tags t
                 JOIN events e ON e.id = t.event_id
                 WHERE t.tag_name = 'e' AND t.tag_value = ?1
                   AND e.kind IN (1, 6, 7, 9735)",
            )?;
            stmt.bind_text(1, &target_hex)?;

            let mut counts = TargetInteractionCounts::default();
            while stmt.step()? {
                let kind = stmt.column_int64(0)? as u32;
                let ev = conv::decode_blob(&stmt.column_blob(1)?)?;
                if let Some((ck, classified)) = classify(kind, &ev.tags) {
                    if classified == target_hex {
                        match ck {
                            CounterKind::Reply => counts.replies += 1,
                            CounterKind::Reaction => counts.reactions += 1,
                            CounterKind::Repost => counts.reposts += 1,
                            CounterKind::Zap => counts.zaps += 1,
                        }
                    }
                }
            }
            Ok(counts)
        }
    }

    /// Lowercase-hex a 32-byte id to its 64-char string (the `event_tags`
    /// tag-value / event-id representation).
    fn to_hex64(id: &EventId) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(64);
        for b in id {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0x0f) as usize] as char);
        }
        s
    }
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

    #[test]
    fn kind1_reply_marker_wins_over_root_and_bare() {
        let tags = vec![etag("bare"), etag_m("rootid", "root"), etag_m("replyid", "reply")];
        let (ck, target) = classify(1, &tags).unwrap();
        assert_eq!(ck, CounterKind::Reply);
        assert_eq!(target, "replyid");
    }

    #[test]
    fn kind1_bare_etag_is_reply_target() {
        let (ck, target) = classify(1, &[etag("deadbeef")]).unwrap();
        assert_eq!(ck, CounterKind::Reply);
        assert_eq!(target, "deadbeef");
    }

    #[test]
    fn reaction_repost_zap_first_etag() {
        assert_eq!(classify(7, &[etag("a")]).unwrap(), (CounterKind::Reaction, "a".into()));
        assert_eq!(classify(6, &[etag("b")]).unwrap(), (CounterKind::Repost, "b".into()));
        assert_eq!(classify(9735, &[etag("c")]).unwrap(), (CounterKind::Zap, "c".into()));
    }

    #[test]
    fn non_interaction_kinds_are_none() {
        assert!(classify(0, &[etag("x")]).is_none());
        assert!(classify(3, &[etag("x")]).is_none());
        assert!(classify(30023, &[etag("x")]).is_none());
        assert!(classify(1, &[]).is_none());
    }
}
