//! Minimal end-to-end example: construct an [`NmpApp`] via `NmpAppBuilder`,
//! inherit the canonical NMP composition, start the kernel, and tear it down.
//!
//! Run with: `cargo run -p nmp-app-template --example minimal_app`
//!
//! This example is intentionally tiny — its load-bearing claims are:
//!
//! 1. `NmpAppBuilder` is the single entry-point for Rust composition roots.
//! 2. `register_defaults` is the one function a new Nostr app calls to get the
//!    standard NMP wiring (NIPs 02/17/57/65, routing substrate, coverage hook,
//!    DM-inbox + zap-receipts runtimes, WOT bootstrap).
//! 3. The builder's typestate enforces that a storage decision is made before
//!    `start()` — if `.in_memory()` (or `.storage_path(p)`) is omitted, the
//!    code does not compile.
//!
//! If this example outgrows ~20 lines of actual work, the template is
//! regressing toward boilerplate.

use nmp_app_template::{NmpAppBuilder, RunConfig};
use nmp_ffi::{nmp_app_free, nmp_app_stop};

fn main() {
    // 1. Start the builder.
    let mut builder = NmpAppBuilder::new();

    // 2. Inherit the canonical NMP composition: NIP-02/17/57/65 action
    //    modules, NIP-17 ingest parser (kind:10050), production routing
    //    substrate (GenericOutboxRouter + InMemoryMailboxCache), D2 coverage
    //    hook, and the DM-inbox + zap-receipts + WOT runtime controllers.
    nmp_app_template::register_defaults(&mut builder);

    // 3. (Optional) Register any app-specific projections / actions here.
    //    e.g. nmp_nip29::register_actions(&mut builder) for group chat.

    // 4. Commit the storage choice and start the kernel.
    //    `.in_memory()` transitions to `NmpAppBuilder<StorageSet>`, unlocking
    //    `.start()`. For production replace with `.storage_path("/path/to/lmdb")`.
    //
    //    Omitting this step is a COMPILE ERROR — V-94 typestate guarantee.
    let app = builder.in_memory().start(RunConfig::default());

    println!("nmp-app-template: NmpAppBuilder → start() complete.");
    println!("  - NIP-02 social actions wired");
    println!("  - NIP-17 DM action + kind:10050 ingest parser wired");
    println!("  - NIP-57 zap action wired");
    println!("  - NIP-65 kind:10002 publish action wired");
    println!("  - GenericOutboxRouter + InMemoryMailboxCache substrate installed");
    println!("  - D2 coverage hook installed");
    println!("  - DM-inbox + zap-receipts + WOT runtime controllers registered");
    println!("  - Kernel started (in-memory store)");

    // 5. Tear down.
    nmp_app_stop(app);
    nmp_app_free(app);
}
