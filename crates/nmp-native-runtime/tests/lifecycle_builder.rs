use nmp_native_runtime::{NmpApp, NmpAppBuilder, RunConfig};

fn free_app(app: *mut NmpApp) {
    assert!(!app.is_null(), "builder returned a null app pointer");
    // SAFETY: `NmpAppBuilder::start` transfers ownership of the pointer to the
    // caller. This test is the owner and drops it exactly once.
    unsafe {
        (&*app).stop_runtime();
        drop(Box::from_raw(app));
    }
}

#[test]
fn builder_drives_native_lifecycle_without_c_abi() {
    let app = NmpAppBuilder::new()
        .in_memory()
        .declare_consumed_projections(["profile"])
        .without_initial_relays()
        .start(RunConfig {
            visible_limit: 16,
            emit_hz: 2,
        });

    // SAFETY: non-null pointer returned from `start`; freed at the end.
    let app_ref = unsafe { &*app };
    app_ref.configure_runtime(8, 1);
    app_ref.lifecycle_foreground();
    app_ref.lifecycle_background();
    app_ref.reset_runtime();
    app_ref.stop_runtime();

    free_app(app);
}
