import Foundation
import FlatBuffers

/// One deduplicated NIP-50 search hit, a 1:1 mirror of the Rust
/// `nmp_nip50::SearchHit` (raw protocol values only, ADR-0032) carried in the
/// typed `N50S` FlatBuffers sidecar. The kernel projects the higher-order
/// `open_search` results onto the per-session key `nmp.nip50.search.<session>`;
/// this struct is what `SearchResultsDecoder` decodes each row into.
///
/// `id` doubles as the dedup key and a stable SwiftUI list identity.
struct ChirpSearchHit: Identifiable, Equatable {
    /// Raw hex event id (dedup key + list identity).
    let id: String
    /// Raw hex author pubkey.
    let author: String
    /// Event kind (0 = profile, 1 = note, 30023 = long-form, …).
    let kind: UInt32
    /// Raw signed `created_at`, Unix seconds.
    let createdAt: UInt64
    /// Raw event content (kind:0 → profile JSON, kind:1 → note text, …).
    let content: String
    /// Raw protocol tags, each an ordered cell list (`["e","<id>",…]`).
    let tags: [[String]]
    /// Relays that delivered this event id (provenance union).
    let relayProvenance: [String]
    /// Dedup provenance: `true` = first arrival was a local cache scan hit.
    let isCache: Bool
    /// The delivering relay when `isCache == false`; empty for a cache hit.
    let sourceRelay: String
}

/// Decodes the per-session typed `N50S` search-results sidecar(s) (schema id
/// `nmp.nip50.search`, file identifier `N50S`) carried on a snapshot frame into
/// the Swift `[sessionId: [ChirpSearchHit]]` model.
///
/// This mirrors `TypedHomeFeedDecoder` exactly: a hand-written decoder that
/// matches typed projection envelopes by `key` + `schemaId` and reads the
/// FlatBuffers payload with the generated `nmp_nip50_*` accessors — there is
/// ZERO search business logic in Swift (the kernel owns query validation,
/// relay selection, cache-FTS, and dedup; ADR-0064 / Chirp thin-shell rule).
///
/// Search projections are SESSION-keyed: the kernel registers one sidecar per
/// live `open_search` session under `nmp.nip50.search.<session_id>`. The
/// `SearchController` pulls one session's `N50S` buffer via the
/// `nmp_app_search_snapshot` C-ABI seam and hands the raw bytes to
/// `decode(bytes:)` — so this decoder never has to scan the whole frame's
/// envelope set; it just maps one buffer to the Swift hit list.
///
/// Like every typed decoder it falls back gracefully: empty / malformed bytes
/// yield an empty list (the host shows "no results yet").
enum SearchResultsDecoder {
    /// FlatBuffers `file_identifier` for `SearchResultsSnapshot`
    /// (`nmp_nip50::wire::FILE_IDENTIFIER`).
    static let fileIdentifier = "N50S"

    /// Decode a raw `N50S` FlatBuffers buffer into the Swift hit list.
    ///
    /// Uses `getRoot` (unchecked) because the buffer crosses a trusted
    /// in-process FFI boundary from the Rust kernel — running the O(N)
    /// FlatBuffers Verifier on every frame is wasted CPU. The `!bytes.isEmpty`
    /// guard at the call site is the only presence check needed.
    static func decode(bytes: Data) -> [ChirpSearchHit] {
        guard !bytes.isEmpty else { return [] }
        var buffer = ByteBuffer(data: bytes)
        let snapshot: nmp_nip50_SearchResultsSnapshot = getRoot(byteBuffer: &buffer)
        return snapshot.hits.map(makeHit)
    }

    private static func makeHit(_ hit: nmp_nip50_SearchHit) -> ChirpSearchHit {
        let tags: [[String]] = hit.tags.map { row in
            row.cells.compactMap { $0 }
        }
        return ChirpSearchHit(
            id: hit.id ?? "",
            author: hit.author ?? "",
            kind: hit.kind,
            createdAt: hit.createdAt,
            content: hit.content ?? "",
            tags: tags,
            relayProvenance: hit.relayProvenance.compactMap { $0 },
            isCache: hit.isCache,
            sourceRelay: hit.sourceRelay ?? ""
        )
    }
}
