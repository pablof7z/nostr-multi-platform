import XCTest
@testable import Chirp

/// Unit tests for `KernelModel.readEligibleRelayUrls(rows:)` — the role-token
/// filter that picks which kernel `RelayEditRow`s should land in the user's
/// NIP-17 kind:10050 DM-relay list.
///
/// Per NIP-17 § 2, kind:10050 lists the relays where the user wants to
/// *receive* gift-wrapped DMs. That maps to the kernel's read-eligible role
/// tokens (`read` or `both`); write-only relays must be excluded.
///
/// These tests pin the Swift filter to the Rust `has_role` semantics in
/// `crates/nmp-core/src/actor/relay_roles.rs`. If the canonical role token
/// vocabulary changes (e.g. a new token replaces `both`), these tests will
/// fail loudly rather than silently mis-publishing every user's DM-relay list.
final class DmRelayListFilterTests: XCTestCase {

    private func row(_ url: String, _ role: String) -> RelayEditRow {
        RelayEditRow(url: url, role: role)
    }

    /// A `read` row is included; a `write`-only row is excluded.
    func testReadIncludedWriteExcluded() {
        let rows = [
            row("wss://read.example", "read"),
            row("wss://write.example", "write"),
        ]
        XCTAssertEqual(
            KernelModel.readEligibleRelayUrls(rows: rows),
            ["wss://read.example"]
        )
    }

    /// `both` counts as read — it's the read+write composite from the Rust
    /// `canonical_relay_role` mapping (`relay_roles.rs:41-44`).
    func testBothIsRead() {
        let rows = [row("wss://both.example", "both")]
        XCTAssertEqual(
            KernelModel.readEligibleRelayUrls(rows: rows),
            ["wss://both.example"]
        )
    }

    /// Composite role strings (`both,indexer`, `read,indexer`) tokenize on
    /// commas. An `indexer`-only row is NOT read-eligible.
    func testCompositeRolesTokenizeOnCommas() {
        let rows = [
            row("wss://composite-both.example", "both,indexer"),
            row("wss://composite-read.example", "read,indexer"),
            row("wss://indexer-only.example", "indexer"),
        ]
        XCTAssertEqual(
            KernelModel.readEligibleRelayUrls(rows: rows),
            [
                "wss://composite-both.example",
                "wss://composite-read.example",
            ]
        )
    }

    /// Tokens are also split on `+` and whitespace, matching the Rust
    /// `role_tokens` iterator (`relay_roles.rs:53-57`). Defensive: callers
    /// may hand-edit role strings or migrate from older formats.
    func testTokenizationOnPlusAndWhitespace() {
        let rows = [
            row("wss://plus.example", "read+indexer"),
            row("wss://space.example", "read indexer"),
        ]
        XCTAssertEqual(
            KernelModel.readEligibleRelayUrls(rows: rows),
            [
                "wss://plus.example",
                "wss://space.example",
            ]
        )
    }

    /// Case-insensitive — Rust's `role_tokens` lowercases each token. A
    /// `READ` row must be included.
    func testCaseInsensitive() {
        let rows = [
            row("wss://upper.example", "READ"),
            row("wss://mixed.example", "Both"),
            row("wss://write-upper.example", "WRITE"),
        ]
        XCTAssertEqual(
            KernelModel.readEligibleRelayUrls(rows: rows),
            [
                "wss://upper.example",
                "wss://mixed.example",
            ]
        )
    }

    /// Empty input yields empty output — the publish helper relies on this
    /// to skip the FFI hand-off (the Rust action rejects empty input).
    func testEmptyInput() {
        XCTAssertEqual(KernelModel.readEligibleRelayUrls(rows: []), [])
    }

    /// All-write input yields empty output — the publish helper short-
    /// circuits on this case so we never clear the cache on every peer
    /// when a user temporarily has only write relays configured.
    func testAllWriteYieldsEmpty() {
        let rows = [
            row("wss://w1.example", "write"),
            row("wss://w2.example", "write"),
        ]
        XCTAssertEqual(KernelModel.readEligibleRelayUrls(rows: rows), [])
    }

    /// Ordering is preserved — the Rust executor canonicalizes and dedupes,
    /// but a stable input order keeps the diffable `Set` comparison in
    /// `KernelModel.maybePublishDmRelayList` honest (set equality is what
    /// gates re-publish; iteration order is incidental).
    func testOrderingPreserved() {
        let rows = [
            row("wss://a.example", "read"),
            row("wss://b.example", "write"),
            row("wss://c.example", "both"),
            row("wss://d.example", "read"),
        ]
        XCTAssertEqual(
            KernelModel.readEligibleRelayUrls(rows: rows),
            [
                "wss://a.example",
                "wss://c.example",
                "wss://d.example",
            ]
        )
    }
}
