//! D16 negative fixture — conformant `nmp.<nip>.*` projection keys and the
//! `// doctrine-allow: D16` opt-out.
//!
//! None of these lines should fire a D16 finding. Lines that contain D0-banned
//! tokens (`nip29`, etc.) carry `// doctrine-allow: D0` to suppress D0 when
//! this fixture is staged outside the exempt path tree.

fn register_conformant(app: &NmpApp) {
    // Fully-prefixed `nmp.` keys are D16-clean.
    app.register_snapshot_projection("nmp.chat.messages", move || snapshot_json());
    app.register_snapshot_projection("nmp.inbox.dms", move || inbox_json());
    app.register_snapshot_projection("nmp.discovery.groups", move || groups_json());
    app.register_snapshot_projection("nmp.dm.relay_list", move || relay_json());

    // A chirp-namespaced key is also fine — D16 only targets `nip17.` / `nip29.`.
    app.register_snapshot_projection("chirp.follow_list", move || follows_json());

    // A legacy bare key with the D16 opt-out.
    let _id = stable_hash(("stable-seed.discover", pubkey)); // doctrine-allow: D16 — stable hash seed, not a projection key
}
