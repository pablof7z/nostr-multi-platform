import Foundation

// Chirp's relay bootstrap, extracted from `KernelModel.start()`.
//
// When NMP_TEST_RELAYS is set (E2E / XCUITest harnesses) that JSON array
// REPLACES the defaults entirely — no merge. Format:
// [["ws://127.0.0.1:10547","both"]] (same shape as Android). Kotlin/Rust own
// parsing on Android; here we do minimal array iteration so the Swift shell
// stays thin. Rust validates each entry on add_relay. When NMP_TEST_RELAYS is
// absent the production defaults are used.
//
// nmp-core no longer carries a hardcoded relay fallback — the app owns its
// default relay set. These pre-start `addRelay` calls populate
// `configured_relays` so the kernel has discovery/content relays on a fresh
// install; the actor dedups them against any session-restored relay list, so
// re-seeding existing rows is a no-op. (Mirrors the Rust `NmpAppBuilder`
// default-relay path.)
func seedChirpRelays(into kernel: KernelHandle) {
    if let testRelaysJson = ProcessInfo.processInfo.environment["NMP_TEST_RELAYS"],
       let data = testRelaysJson.data(using: .utf8),
       let entries = try? JSONSerialization.jsonObject(with: data) as? [[String]],
       !entries.isEmpty {
        for entry in entries where entry.count == 2 {
            kernel.addRelay(url: entry[0], role: entry[1])
        }
    } else {
        // Default Chirp relay bootstrap (mirrors nmp-chirp-config).
        kernel.addRelay(url: "wss://r.f7z.io", role: "both")
        kernel.addRelay(url: "wss://purplepag.es", role: "indexer")
    }
}
