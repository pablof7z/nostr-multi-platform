//! NIP-10 reply-tag construction for kind:1 publishes (T144).
//!
//! Owns two crate-internal `Kernel` methods called by
//! `actor::commands::publish::publish_note`:
//!
//! - [`Kernel::reply_tags_for_parent`] — look the parent up in the local
//!   `events` cache, decode its NIP-10 refs via [`crate::tags::parse_nip10`],
//!   and build the full marked-form reply tag set (root forwarding,
//!   parent-author re-notification, mentioned-pubkey forwarding, dedup).
//!   Returns `None` if the parent isn't cached — the caller falls back to a
//!   minimal reply marker and kicks hydration.
//!
//! - [`Kernel::kick_thread_hydration`] — enqueue the missing parent id onto
//!   the existing T121 thread-hydration queue and drive it. Reuses the
//!   `pending_thread_ids` / `requested_thread_ids` machinery so the REQ
//!   partitions by NIP-65 outbox the same way every other hydration does.
//!
//! ## Why this lives in `nmp-core` and not `nmp-nip01`
//!
//! `nmp-nip01` already depends on `nmp-core` (its `decode::NoteRecord` /
//! `build::Note` types sit on `nmp_core::tags` + `nmp_core::substrate`).
//! Adding `nmp-core → nmp-nip01` would cycle. The output of this module is
//! byte-identical to `nmp_nip01::Note::reply_to` because it composes the
//! same `crate::tags::{e_tag, p_tag, parse_nip10}` primitives that builder
//! is composed of (see PD-024 in `docs/perf/pending-user-decisions.md`).

use super::Kernel;
use crate::relay::OutboundMessage;
use crate::tags::{e_tag, p_tag, parse_nip10};

impl Kernel {
    /// Build the full NIP-10 marked-form tag set for replying to `parent_id`,
    /// returning `None` if the parent isn't in the local `events` cache.
    ///
    /// The emitted tags mirror `nmp_nip01::Note::reply_to`'s output exactly:
    ///
    /// - One `["e", root_id, root_relay_or_empty, "root"]` — promoting parent
    ///   if parent had no root of its own.
    /// - One `["e", parent_id, "", "reply"]`.
    /// - One `["p", parent_author]` plus one `p` tag per pubkey the parent was
    ///   already notifying, de-duplicated, stable order.
    ///
    /// Relay-hint slots stay empty; the kernel has no per-event relay provenance
    /// to thread through. Deliberate parity with the existing builder which also
    /// defaults relays to `None`.
    pub(crate) fn reply_tags_for_parent(&self, parent_id: &str) -> Option<Vec<Vec<String>>> {
        let parent = self.events.get(parent_id)?;
        // The parent must itself be a kind:1 note for "reply to it as a note"
        // to be meaningful. Anything else (a kind:30023 article, a kind:6
        // repost, …) is caller error; refuse rather than emit malformed tags.
        if parent.kind != 1 {
            return None;
        }

        let parent_refs = parse_nip10(&parent.tags);

        // Root forwarding: if the parent has its own `root` ref, carry it
        // through; otherwise the parent itself is the thread root.
        let (root_id, root_relay) = match parent_refs.root.as_ref() {
            Some(root) => (root.id.clone(), root.relay.clone()),
            None => (parent_id.to_string(), None),
        };

        // P-tag set: parent author first, then parent's `mentioned_pubkeys`,
        // de-duplicated, stable order — same shape as `Note::reply_to`.
        let mut pubkeys: Vec<String> = Vec::with_capacity(1 + parent_refs.mentioned_pubkeys.len());
        pubkeys.push(parent.author.clone());
        for pk in &parent_refs.mentioned_pubkeys {
            if !pubkeys.iter().any(|p| p == pk) {
                pubkeys.push(pk.clone());
            }
        }

        let mut tags: Vec<Vec<String>> = Vec::with_capacity(2 + pubkeys.len());
        tags.push(e_tag(&root_id, root_relay.as_deref(), Some("root")));
        tags.push(e_tag(parent_id, None, Some("reply")));
        for pk in pubkeys {
            tags.push(p_tag(&pk, None));
        }
        Some(tags)
    }

    /// Enqueue `id` onto the T121 thread-hydration queue and drive it. Used
    /// by the cold-reply path in `publish_note` so a reply to an unknown
    /// parent issues a one-shot REQ to fetch the parent's structure.
    ///
    /// Reuses the existing `pending_thread_ids` → `maybe_open_thread_hydration`
    /// machinery — the REQ partitions by NIP-65 outbox identically to every
    /// other thread-hydration REQ.
    pub(crate) fn kick_thread_hydration(&mut self, id: String) -> Vec<OutboundMessage> {
        self.enqueue_thread_id(id);
        self.maybe_open_thread_hydration()
    }
}

#[cfg(test)]
mod tests {
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::tags::{e_tag, p_tag};

    /// Deterministic 64-char hex fixtures — no `SystemTime`, no randomness, so
    /// every assertion is exact and reproducible (memory: "no polling / no
    /// non-determinism" + task brief: deterministic fixtures only).
    fn root_id() -> String {
        "aa".repeat(32)
    }
    fn parent_id() -> String {
        "bb".repeat(32)
    }
    fn root_author() -> String {
        "cc".repeat(32)
    }
    fn parent_author() -> String {
        "dd".repeat(32)
    }
    fn third_party() -> String {
        "ee".repeat(32)
    }

    /// Pull the row for the first tag whose marker (column 3) equals `marker`.
    fn marked_e_tag<'a>(tags: &'a [Vec<String>], marker: &str) -> &'a Vec<String> {
        tags.iter()
            .find(|t| {
                t.first().map(String::as_str) == Some("e")
                    && t.get(3).map(String::as_str) == Some(marker)
            })
            .unwrap_or_else(|| panic!("no e-tag with marker {marker:?}"))
    }

    /// All `p`-tag pubkeys (column 2), in document order.
    fn p_values(tags: &[Vec<String>]) -> Vec<&str> {
        tags.iter()
            .filter(|t| t.first().map(String::as_str) == Some("p"))
            .filter_map(|t| t.get(1))
            .map(String::as_str)
            .collect()
    }

    /// Replying to a flat root-level post (the parent carries no `e` tags of
    /// its own): the parent becomes BOTH the `root` and the `reply` marker.
    #[test]
    fn reply_to_root_promotes_parent_to_root_and_reply() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // A genuine thread root: kind:1, no `e` tags.
        kernel.seed_kind1_for_reply_test(
            &parent_id(),
            &parent_author(),
            1_000,
            vec![],
            "root post",
        );

        let tags = kernel
            .reply_tags_for_parent(&parent_id())
            .expect("cached kind:1 parent must yield reply tags");

        // root marker → the parent itself (it had no root of its own).
        let root = marked_e_tag(&tags, "root");
        assert_eq!(root, &e_tag(&parent_id(), None, Some("root")));
        // reply marker → also the parent.
        let reply = marked_e_tag(&tags, "reply");
        assert_eq!(reply, &e_tag(&parent_id(), None, Some("reply")));
        // Parent author is re-notified.
        assert_eq!(p_values(&tags), vec![parent_author().as_str()]);
    }

    /// Replying to a reply that already carries a `root` marker: that root is
    /// forwarded unchanged, and the parent we are replying to becomes the new
    /// `reply` marker.
    #[test]
    fn reply_to_reply_forwards_existing_root() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // The parent is itself a reply: it has a marked `root` + `reply` tag set.
        let parent_tags = vec![
            e_tag(&root_id(), None, Some("root")),
            e_tag(&"ff".repeat(32), None, Some("reply")),
            p_tag(&root_author(), None),
        ];
        kernel.seed_kind1_for_reply_test(
            &parent_id(),
            &parent_author(),
            2_000,
            parent_tags,
            "a reply",
        );

        let tags = kernel
            .reply_tags_for_parent(&parent_id())
            .expect("cached kind:1 parent must yield reply tags");

        // root marker → forwarded from the parent's own root, NOT the parent id.
        let root = marked_e_tag(&tags, "root");
        assert_eq!(root, &e_tag(&root_id(), None, Some("root")));
        // reply marker → the parent we are replying to.
        let reply = marked_e_tag(&tags, "reply");
        assert_eq!(reply, &e_tag(&parent_id(), None, Some("reply")));
        // p-tags: parent author first, then the forwarded thread participant.
        assert_eq!(
            p_values(&tags),
            vec![parent_author().as_str(), root_author().as_str()],
        );
    }

    /// Edge case: a parent whose `e` tags exist but carry NO `root` marker is
    /// treated as a thread root — the parent itself is promoted to `root`.
    #[test]
    fn reply_to_parent_without_root_marker_treats_parent_as_root() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // Parent has only a `mention`-marked e-tag: no `root`, no `reply`.
        let parent_tags = vec![e_tag(&"99".repeat(32), None, Some("mention"))];
        kernel.seed_kind1_for_reply_test(
            &parent_id(),
            &parent_author(),
            3_000,
            parent_tags,
            "quote post",
        );

        let tags = kernel
            .reply_tags_for_parent(&parent_id())
            .expect("cached kind:1 parent must yield reply tags");

        // No usable root on the parent → the parent becomes the root.
        let root = marked_e_tag(&tags, "root");
        assert_eq!(root, &e_tag(&parent_id(), None, Some("root")));
        let reply = marked_e_tag(&tags, "reply");
        assert_eq!(reply, &e_tag(&parent_id(), None, Some("reply")));
    }

    /// A relay hint stored on the parent's own `root` marker is forwarded onto
    /// the generated `root` e-tag; the `reply` e-tag's relay slot stays empty
    /// (a deliberate parity choice documented on `reply_tags_for_parent`).
    #[test]
    fn relay_hint_on_parent_root_is_forwarded() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let parent_tags = vec![
            e_tag(&root_id(), Some("wss://relay.example"), Some("root")),
            e_tag(&"ff".repeat(32), None, Some("reply")),
        ];
        kernel.seed_kind1_for_reply_test(
            &parent_id(),
            &parent_author(),
            4_000,
            parent_tags,
            "deep reply",
        );

        let tags = kernel
            .reply_tags_for_parent(&parent_id())
            .expect("cached kind:1 parent must yield reply tags");

        // root e-tag carries the forwarded relay hint in its relay slot.
        let root = marked_e_tag(&tags, "root");
        assert_eq!(
            root,
            &e_tag(&root_id(), Some("wss://relay.example"), Some("root")),
        );
        assert_eq!(root.get(2).map(String::as_str), Some("wss://relay.example"));
        // reply e-tag's relay slot is empty by design.
        let reply = marked_e_tag(&tags, "reply");
        assert_eq!(reply.get(2).map(String::as_str), Some(""));
    }

    /// A parent that isn't in the local `events` cache yields `None` so the
    /// caller can fall back to a minimal reply marker and kick hydration.
    #[test]
    fn uncached_parent_yields_none() {
        let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        assert!(kernel.reply_tags_for_parent(&parent_id()).is_none());
    }

    /// Parent `p`-tags are forwarded after the parent author, de-duplicated,
    /// in stable document order — re-notifying every thread participant once.
    #[test]
    fn parent_pubkeys_are_forwarded_and_deduped() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // Parent notifies: third_party, parent_author (dup of self), third_party (dup).
        let parent_tags = vec![
            e_tag(&root_id(), None, Some("root")),
            e_tag(&"ff".repeat(32), None, Some("reply")),
            p_tag(&third_party(), None),
            p_tag(&parent_author(), None),
            p_tag(&third_party(), None),
        ];
        kernel.seed_kind1_for_reply_test(
            &parent_id(),
            &parent_author(),
            5_000,
            parent_tags,
            "busy thread",
        );

        let tags = kernel
            .reply_tags_for_parent(&parent_id())
            .expect("cached kind:1 parent must yield reply tags");

        // parent author first, then each distinct mentioned pubkey exactly once.
        assert_eq!(
            p_values(&tags),
            vec![parent_author().as_str(), third_party().as_str()],
        );
    }
}
