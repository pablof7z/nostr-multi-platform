//! Kind:3 (contact list) ingest.

use super::super::*;
use crate::subs::{AccountId, CompileTrigger};

impl Kernel {
    /// Ingest a kind:3 contact-list event into the local `seed_contacts` cache
    /// and fan a `FollowListChanged` (A11) trigger into the subscription
    /// lifecycle inbox.
    ///
    /// Only called after `verify_and_persist` returns `Inserted | Replaced` (D4).
    /// Extracts "p"-tagged hex pubkeys, capping at `TIMELINE_AUTHOR_LIMIT`.
    ///
    /// The A11 trigger causes `drain_tick` (on the next tick boundary) to run
    /// a recompile so any ViewModule whose `dependencies()` declares `kind 3`
    /// picks up the new follow-set without app involvement. The kernel's M1
    /// hand-rolled `req()` path continues to drive the wire; the lifecycle
    /// trigger is the seam that M2 phase-2 / M11 will route to the compiler
    /// once ViewModules are wired onto `LogicalInterest`.
    ///
    /// Seam-gap note: the actor loop must call `lifecycle.drain_tick()` at each
    /// tick boundary for this trigger to produce wire frames in production.
    /// Today the kernel uses the lifecycle only for the AuthGate
    /// (`handle_auth_state_change`); the compile / registry machinery is dormant
    /// until M11 migrates view modules onto `LogicalInterest`.
    pub(in crate::kernel) fn ingest_contacts(&mut self, event: NostrEvent) {
        let follows = event
            .tags
            .iter()
            .filter_map(|tag| {
                if tag.first().map(String::as_str) == Some("p") {
                    tag.get(1).filter(|value| is_hex_pubkey(value)).cloned()
                } else {
                    None
                }
            })
            .take(TIMELINE_AUTHOR_LIMIT)
            .collect::<Vec<_>>();

        self.log(format!(
            "contacts {} -> {} followees",
            short_hex(&event.pubkey),
            follows.len()
        ));

        // A11: fan a FollowListChanged trigger into the lifecycle inbox so the
        // subscription compiler recompiles on the next tick. Per D8, multiple
        // kind:3 arrivals within one tick collapse to a single compile pass.
        self.lifecycle
            .enqueue_trigger(CompileTrigger::FollowListChanged {
                account_id: AccountId(event.pubkey.clone()),
                new_follows: follows.clone(),
            });

        self.seed_contacts.insert(event.pubkey, follows);
    }
}
