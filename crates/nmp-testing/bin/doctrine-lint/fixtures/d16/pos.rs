// D16 positive fixture — bare `nip29.*` / `nip17.*` projection-key literals.
//
// Both lines below MUST fire a D16 finding. The docstring key `"nip29.group_chat"`
// and the `"nip17.dm_inbox"` registration are the shapes the rule targets.

fn register_group_chat(app: &NmpApp) {
    app.register_snapshot_projection("nip29.group_chat", move || snapshot_json());
    app.register_snapshot_projection("nip17.dm_inbox", move || inbox_json());
}
