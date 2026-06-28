import XCTest

/// iOS counterpart to Android's `ComponentVendorDriftGateTest`.
///
/// The identicon algorithm (`NostrIdenticon`: djb2 hash → HSB color +
/// 5×5 symmetric cell grid) is the single source of truth that #2224 unifies.
/// On iOS the enum is declared in several Swift files because the registry
/// ships it from two independently-installable components and apps vendor
/// their own copies:
///
///   1. registry `swiftui/user-avatar/NostrAvatar.swift`     (standalone install)
///   2. registry `swiftui/content-core/ContentTreeWire.swift` (content install)
///   3. Chirp     `…/NostrContent/ContentTreeWire.swift`      (vendored copy)
///   4. nmp-gallery `…/Registry/ContentTreeWire.swift`        (vendored copy)
///
/// Unlike the Android `NostrAvatar.kt` whole-file gate, the iOS files are NOT
/// whole-file identical (CodingKey camelCase rules and ADR-0063 reactive
/// additions legitimately differ between them). What MUST stay identical is the
/// `enum NostrIdenticon { … }` declaration — the rendering algorithm itself.
/// This gate extracts that brace-balanced block from each file and asserts
/// byte-equality, so the four copies can never silently diverge. That silent
/// divergence is exactly the failure #2224 exists to prevent.
///
/// Mechanism: `#filePath` resolves to this source file's absolute path at
/// compile time. The iOS simulator shares the host filesystem, so the test can
/// walk up to the repo root and read the registry/app sources directly — the
/// same approach snapshot-testing libraries use to locate host files from a
/// simulator test bundle.
final class IdenticonVendorDriftGateTest: XCTestCase {

    /// Repo root, located by walking up from this test file until the marker
    /// files (`Cargo.lock` + `crates/nmp-cli`) are found. Mirrors the Android
    /// gate's `repoRoot` discovery.
    private func repoRoot() throws -> URL {
        var dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        let fm = FileManager.default
        for _ in 0..<32 {
            let cargoLock = dir.appendingPathComponent("Cargo.lock")
            let nmpCli = dir.appendingPathComponent("crates/nmp-cli")
            if fm.fileExists(atPath: cargoLock.path),
               fm.fileExists(atPath: nmpCli.path) {
                return dir
            }
            let parent = dir.deletingLastPathComponent()
            if parent.path == dir.path { break }
            dir = parent
        }
        throw XCTSkip("repo root not found above \(#filePath); skipping source-drift gate")
    }

    /// Extracts the `enum NostrIdenticon { … }` brace-balanced block from a
    /// Swift source file as a normalized string (trailing whitespace trimmed
    /// per line so editor settings can't cause a spurious failure).
    private func identiconBlock(_ relativePath: String, root: URL) throws -> String {
        let url = root.appendingPathComponent(relativePath)
        let source = try String(contentsOf: url, encoding: .utf8)
        let lines = source.components(separatedBy: "\n")

        var captured: [String] = []
        var depth = 0
        var started = false
        for line in lines {
            if !started && line.contains("enum NostrIdenticon {") {
                started = true
            }
            guard started else { continue }
            captured.append(line)
            depth += line.filter { $0 == "{" }.count
            depth -= line.filter { $0 == "}" }.count
            if depth == 0 { break }
        }

        XCTAssertFalse(
            captured.isEmpty,
            "no `enum NostrIdenticon {` block found in \(relativePath)"
        )
        XCTAssertEqual(
            depth, 0,
            "unbalanced braces extracting NostrIdenticon from \(relativePath)"
        )
        // Trim trailing whitespace per line — defensive against editor config.
        return captured
            .map { $0.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression) }
            .joined(separator: "\n")
    }

    /// The registry source is canonical; every other copy must match it byte
    /// for byte (modulo trailing whitespace).
    private let canonicalPath =
        "crates/nmp-cli/registry/swiftui/content-core/ContentTreeWire.swift"

    private let copyPaths = [
        "crates/nmp-cli/registry/swiftui/user-avatar/NostrAvatar.swift",
        "apps/chirp/ios/Chirp/Components/NostrContent/ContentTreeWire.swift",
        "apps/nmp-gallery/ios/NmpGallery/Registry/ContentTreeWire.swift",
    ]

    func testNostrIdenticonAlgorithmIsByteIdenticalAcrossAllCopies() throws {
        let root = try repoRoot()
        let canonical = try identiconBlock(canonicalPath, root: root)

        for copyPath in copyPaths {
            let copy = try identiconBlock(copyPath, root: root)
            XCTAssertEqual(
                copy,
                canonical,
                """
                NostrIdenticon algorithm drifted between
                  \(canonicalPath)
                and
                  \(copyPath)
                The 5×5 identicon algorithm MUST stay byte-identical across all
                copies (#2224). If you changed the algorithm, change every copy
                in the same commit.
                """
            )
        }
    }
}
