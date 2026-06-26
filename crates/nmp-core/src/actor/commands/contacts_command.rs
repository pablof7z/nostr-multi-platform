//! `ContactsCommand` — active-account kind:3 follow-set publishing (ADR-0065).
//!
//! Grouped under `ActorCommand::Contacts(ContactsCommand)`. Dispatch home:
//! `actor/dispatch/cmd_publish.rs` (kind:3 follow-set path).

/// Active-account kind:3 follow-set mutations.
///
/// Each variant reads the current kind:3, mutates the p-tag set, and
/// re-publishes ONE signed kind:3 via the publish engine. The dispatch arm
/// routes through `cmd_publish::follow_or_unfollow` / `cmd_publish::follow_many`.
#[derive(Debug)]
pub enum ContactsCommand {
    /// T66a publish — append `pubkey` to the active account's kind:3 follow
    /// set and re-publish it.
    Follow {
        pubkey: String,
        /// Registry-minted action id when this Follow originates from
        /// `nmp_app_dispatch_action` (`nmp.follow`). See `React` for the
        /// spinner round-trip contract.
        correlation_id: Option<String>,
    },
    /// T66a publish — remove `pubkey` from the kind:3 follow set.
    Unfollow {
        pubkey: String,
        /// Registry-minted action id when this Unfollow originates from
        /// `nmp_app_dispatch_action` (`nmp.unfollow`). See `React` for the
        /// spinner round-trip contract.
        correlation_id: Option<String>,
    },
    /// Bulk follow — append the full `pubkeys` set to the active account's
    /// kind:3 and re-publish it ONCE.
    ///
    /// Unlike dispatching N sequential [`Self::Follow`] commands (which race:
    /// each reads the current kind:3 before the prior signed event is ingested,
    /// so each publishes a kind:3 with only +1 p-tag and last-write-wins
    /// silently drops every follow but the last), this command reads kind:3
    /// EXACTLY ONCE, folds all target pubkeys via `kind3_tags_after_add` in a
    /// single pass, and produces ONE signed kind:3. The race is structurally
    /// impossible: a single command = a single actor-thread execution slot =
    /// one atomic read-modify-write.
    ///
    /// Invalid (non-64-hex) entries and the active account's own pubkey are
    /// silently dropped; the remaining set is deduped preserving document
    /// order (idempotent adds keep the first occurrence).
    FollowMany {
        pubkeys: Vec<String>,
        /// Registry-minted action id forwarded to the publish-engine terminal
        /// verdict. Matches the value returned by `nmp_app_dispatch_action` so
        /// the host spinner closes on the `nmp.follow_many` action.
        correlation_id: Option<String>,
    },
}
