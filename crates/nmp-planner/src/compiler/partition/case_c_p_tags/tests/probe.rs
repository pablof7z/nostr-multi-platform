//! Defect 1 regression — the bootstrap-inbox path must fire
//! `request_probe` for every tagged pubkey so NIP-65 inbox discovery can
//! eventually re-route the interest off the cold-start bootstrap pad.

use super::{p_tag_interest, pk};
use crate::{
    compiler::{MailboxCache, MailboxSnapshot, SubscriptionCompiler},
    interest::{InterestLifecycle, InterestScope, PTagRouting, Pubkey},
};
use std::collections::BTreeSet;
use std::sync::Mutex;

/// A `MailboxCache` that records every `request_probe` call so tests can
/// assert inbox discovery actually fires. `get`/`dm_inbox_relays` always
/// return `None` (no cached inbox) so the cold-start bootstrap gate is the
/// path under test. `Mutex` (not `RefCell`) because the trait bound is
/// `Send + Sync`.
#[derive(Default)]
struct ProbeRecordingCache {
    probed: Mutex<BTreeSet<Pubkey>>,
}

impl ProbeRecordingCache {
    fn probed(&self) -> BTreeSet<Pubkey> {
        self.probed.lock().expect("probe lock").clone()
    }
}

impl MailboxCache for ProbeRecordingCache {
    fn get(&self, _pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        None
    }
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        Vec::new()
    }
    fn generation(&self) -> u64 {
        0
    }
    fn request_probe(&self, pubkey: &Pubkey) {
        self.probed
            .lock()
            .expect("probe lock")
            .insert(pubkey.clone());
    }
}

/// Defect 1 regression — the bootstrap-inbox path MUST call
/// `mailbox_cache.request_probe(pk)` for EVERY tagged pubkey. Without the
/// probe, NIP-65 inbox discovery never fires and the cold-start bootstrap
/// relay stays sticky forever (DM-inbox discovery broken): the next
/// recompile has no kind:10002 to re-route on, so the interest is stuck on
/// the shared bootstrap pad indefinitely.
///
/// The bootstrap-inbox gate is only reached when EVERY tagged pubkey lacks a
/// cached inbox, so EVERY tagged pubkey must be probed.
#[test]
fn pd033c_bootstrap_inbox_probes_every_tagged_pubkey() {
    let cache = ProbeRecordingCache::default();
    let bootstrap_content = vec!["wss://bootstrap.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &bootstrap_content,
        &[],
    );

    // Two tagged pubkeys, neither with a cached inbox → bootstrap path.
    let interest = p_tag_interest(
        1,
        &["bob", "carol"],
        PTagRouting::Nip65ReadRelays,
        InterestLifecycle::Tailing,
        InterestScope::Global,
    );

    let plan = compiler.compile(&[interest]).expect("compile");

    // The bootstrap lane still carries the REQ (the existing contract).
    assert!(
        plan.per_relay.contains_key("wss://bootstrap.example"),
        "bootstrap content relay must still carry the #p Tailing REQ"
    );

    // …AND a probe was requested for BOTH tagged pubkeys so the next
    // recompile can re-route off the bootstrap pad onto real inbox relays.
    let probed = cache.probed();
    assert!(
        probed.contains(&pk("bob")),
        "bootstrap-inbox path must request_probe(bob); got {probed:?}"
    );
    assert!(
        probed.contains(&pk("carol")),
        "bootstrap-inbox path must request_probe(carol); got {probed:?}"
    );
    assert_eq!(
        probed.len(),
        2,
        "exactly the two tagged pubkeys must be probed; got {probed:?}"
    );
}
