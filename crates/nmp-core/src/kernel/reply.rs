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
    /// Relay-hint slots stay empty for now; the kernel has no per-event relay
    /// provenance to thread through. This is a deliberate parity choice with
    /// the existing builder which also defaults relays to `None`.
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
