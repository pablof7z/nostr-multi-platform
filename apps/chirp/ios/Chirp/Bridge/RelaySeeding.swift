import Foundation

// Chirp's relay bootstrap, extracted from `KernelModel.start()`.
//
// Policy lives in Rust (`nmp-chirp-config`, surfaced via the
// `nmp_app_chirp_seed_*` C-ABI symbols), not in Swift (D7 / thin-shell). The
// shell's ONLY job here is env plumbing: read `NMP_TEST_RELAYS` and hand the
// raw JSON to Rust. The default relay URL set and the override JSON parsing
// both live in `nmp-chirp-config` / `nmp-app-chirp`, mirroring the Android
// `nmp-android-ffi::relay_seeding` glue — so neither shell hardcodes relay
// URLs (the Swift list had already drifted from the canonical config).
//
// When `NMP_TEST_RELAYS` is set (E2E / XCUITest harnesses) that JSON array
// REPLACES the defaults entirely — no merge. Format:
// [["ws://127.0.0.1:10547","both"]] (same shape as Android). Rust parses and
// validates each entry; a malformed or empty override falls back to the
// production defaults. When `NMP_TEST_RELAYS` is absent the defaults are used.
//
// These pre-start `seed*` calls populate `configured_relays` so the kernel has
// discovery/content relays on a fresh install; the actor dedups them against
// any session-restored relay list, so re-seeding existing rows is a no-op.
func seedChirpRelays(into kernel: KernelHandle) {
    // Env plumbing stays in Swift; URL list + JSON parsing live in Rust.
    if let testRelaysJson = ProcessInfo.processInfo.environment["NMP_TEST_RELAYS"],
       kernel.seedRelays(fromJSON: testRelaysJson) {
        return
    }
    // No override (or the override was malformed/empty) → production defaults.
    kernel.seedDefaultRelays()
}
