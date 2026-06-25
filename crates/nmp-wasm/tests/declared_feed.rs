use nmp_wasm::RawWasmAbiAdapter;

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";

#[test]
fn runtime_declares_active_follows_feed_from_primary_kinds() {
    let runtime = RawWasmAbiAdapter::new();

    assert!(
        runtime.declare_active_follows_feed([1]),
        "primary kind 1 is a valid app-facing feed declaration"
    );
    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());

    let authors = runtime.reducer_handle().borrow().active_timeline_authors();
    assert_eq!(
        authors,
        vec![ALICE.to_string()],
        "pre-sign-in declaration must prime the active-follows feed so sign-in \
         installs the active account as the first author"
    );
}

#[test]
fn runtime_rejects_repost_wrappers_as_primary_feed_kinds() {
    let runtime = RawWasmAbiAdapter::new();

    assert!(
        !runtime.declare_active_follows_feed([1, 6]),
        "kind 6 is derived from primary kind 1; apps must not declare it"
    );
    assert!(
        !runtime.declare_active_follows_feed([16]),
        "kind 16 is derived for non-kind-1 primaries; apps must not declare it"
    );
    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());

    let authors = runtime.reducer_handle().borrow().active_timeline_authors();
    assert!(
        authors.is_empty(),
        "rejected wrapper-kind declarations must leave the active-follows feed inert"
    );
}

#[test]
fn runtime_clears_active_follows_feed_declaration() {
    let runtime = RawWasmAbiAdapter::new();

    assert!(runtime.declare_active_follows_feed([1]));
    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());
    assert!(
        !runtime
            .reducer_handle()
            .borrow()
            .active_timeline_authors()
            .is_empty(),
        "test setup must install the active-follows feed before clearing"
    );

    runtime.clear_active_follows_feed();
    assert!(
        runtime
            .reducer_handle()
            .borrow()
            .active_timeline_authors()
            .is_empty(),
        "clear must withdraw the active-follows declaration"
    );
}
