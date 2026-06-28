//! Minimal end-to-end example: construct an [`NmpApp`] via `NmpAppBuilder`,
//! inherit the canonical NMP composition, start the kernel, and tear it down.
//!
//! Run with: `cargo run -p nmp-defaults --example minimal_app`
//!
//! This example is intentionally tiny — its load-bearing claims are:
//!
//! 1. `NmpAppBuilder` is the single entry-point for Rust composition roots.
//! 2. `register_defaults` is the one function a new Nostr app calls to get the
//!    standard NMP wiring (NIPs 02/17/65, routing substrate, coverage hook,
//!    DM-inbox runtime, WOT bootstrap).
//! 3. The builder's typestate enforces that a storage decision AND an
//!    ADR-0053 projection-consumption decision are both made before `start()`
//!    — if `.in_memory()` (or `.storage_path(p)`) or the projection step
//!    (`.declare_consumed_projections` / `.consume_all_builtin_projections`)
//!    is omitted, the code does not compile.
//!
//! If this example outgrows ~20 lines of actual work, the template is
//! regressing toward boilerplate.

use nmp_native_runtime::{NmpAppBuilder, RunConfig};

fn main() {
    // 1. Start the builder.
    let mut builder = NmpAppBuilder::new();

    // 2. Inherit the canonical NMP composition: NIP-02/17/65 action
    //    modules, NIP-17 ingest parser (kind:10050), production routing
    //    substrate (GenericOutboxRouter + InMemoryMailboxCache), D2 coverage
    //    hook, and the DM-inbox + WOT runtime controllers.
    nmp_defaults::register_defaults(&mut builder);

    // 3. (Optional) Register any app-specific projections / actions / search
    //    scopes here. A group-chat app opts into NIP-29 group-metadata
    //    full-text search (#1811) with
    //    `nmp_nip29::register_search_scopes(&builder)` — NIP-29 is a leaf-app
    //    feature, NOT part of the default bundle, so its `nip29.groups`
    //    cache-only scope is wired here rather than in `register_defaults`.

    // 4. Commit the storage choice, declare the consumed projections, decide
    //    the initial relay set, and start the kernel. `.in_memory()` advances
    //    to `StorageSet`; `.consume_all_builtin_projections()` makes the
    //    ADR-0053 decision and advances to `ProjectionsDeclared`;
    //    `.without_initial_relays()` makes the #1493 relay decision (this demo
    //    ships no built-in relays — declare your own with
    //    `.with_relays([("wss://your.relay", "both")])`) and advances to
    //    `RelaysDeclared`, unlocking `.start()`. For production replace
    //    `.in_memory()` with `.storage_path("/path/to/lmdb")`, prefer
    //    `.declare_consumed_projections([..])` to narrow to the keys your UI
    //    reads, and declare your relays with `.with_relays([..])`.
    //
    //    Omitting ANY decision is a COMPILE ERROR — storage = V-94,
    //    projections = ADR-0053 DEBT 2, relays = #1493.
    let app = builder
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .start(RunConfig::default());

    println!("nmp-defaults: NmpAppBuilder → start() complete.");
    println!("  - NIP-02 social actions wired");
    println!("  - NIP-17 DM action + kind:10050 ingest parser wired");
    println!("  - NIP-65 kind:10002 publish action wired");
    println!("  - GenericOutboxRouter + InMemoryMailboxCache substrate installed");
    println!("  - D2 coverage hook installed");
    println!("  - DM-inbox + WOT runtime controllers registered");
    println!("  - Kernel started (in-memory store)");

    // 5. Tear down.
    if !app.is_null() {
        // SAFETY: `start()` returned ownership of this pointer to the example.
        unsafe {
            (&*app).stop_runtime();
            drop(Box::from_raw(app));
        }
    }
}
