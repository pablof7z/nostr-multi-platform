//! Minimal end-to-end example: construct an [`NmpApp`], inherit the
//! canonical NMP composition through one call, and tear it down.
//!
//! Run with: `cargo run -p nmp-app-template --example minimal_app`
//!
//! The example is intentionally tiny — its load-bearing claim is that
//! `register_defaults` is the *single* function a new Nostr app calls to
//! get the standard NMP wiring. If this example outgrows ten lines of
//! actual work, the template is regressing.

use nmp_ffi::{nmp_app_free, nmp_app_new};

fn main() {
    // 1. Construct the app. (No callbacks set — this example only proves
    //    the registration path; a real host wires
    //    `nmp_app_set_update_callback` etc. before `nmp_app_start`.)
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // 2. Inherit the canonical NMP composition: NIP-02/17/57/65 action
    //    modules, NIP-17 ingest parser (kind:10050), production routing
    //    substrate (`GenericOutboxRouter` + `InMemoryMailboxCache`), D2
    //    coverage hook, and the DM-inbox + zap-receipts runtime
    //    controllers.
    //
    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    nmp_app_template::register_defaults(unsafe { &mut *app });

    println!("nmp-app-template: register_defaults complete.");
    println!("  - NIP-02 social actions wired");
    println!("  - NIP-17 DM action + kind:10050 ingest parser wired");
    println!("  - NIP-57 zap action wired");
    println!("  - NIP-65 kind:10002 publish action wired");
    println!("  - GenericOutboxRouter + InMemoryMailboxCache substrate installed");
    println!("  - D2 coverage hook installed");
    println!("  - DM-inbox + zap-receipts runtime controllers registered");

    // 3. Tear down. A real host would call `nmp_app_start` here, drive
    //    the event loop, and call `nmp_app_free` on shutdown.
    nmp_app_free(app);
}
