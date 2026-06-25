import Foundation

/// Typed Swift mirrors of the input-intent resolver (#1804) FFI wire JSON.
///
/// These are pure data-transfer types: they carry NO logic, only `Codable`
/// shapes that match the Rust serde reprs 1:1 (`InputIntentRequest`,
/// `InputScopeId`, and the `nmp_app_intent_dispatch` /
/// `nmp_nip21_decode_uri` result envelopes). The classification decision lives
/// entirely in Rust; Swift switches on the decoded variant and navigates.

// ── Request ──────────────────────────────────────────────────────────────────

/// A registered input scope id — the `(namespace, name)` pair the resolver
/// matches against (e.g. `nostr.ref`, `nip50.profiles`, `nip50.notes`).
/// Serializes as `{"namespace":"…","name":"…"}` to match
/// `nmp_core::substrate::InputScopeId`.
struct IntentScope: Encodable {
    let namespace: String
    let name: String

    /// The synthetic always-allowed direct-reference scope (`nostr.ref`).
    static let nostrRef = IntentScope(namespace: "nostr", name: "ref")
    /// NIP-50 profile (kind:0) free-text search scope (`nip50.profiles`).
    static let nip50Profiles = IntentScope(namespace: "nip50", name: "profiles")
    /// NIP-50 notes (kind:1) free-text search scope (`nip50.notes`). This is the
    /// registered kind:1 content scope; there is no `nip50.content` scope.
    static let nip50Notes = IntentScope(namespace: "nip50", name: "notes")
}

/// The free-text search-target choice (`UserPreferred` / `AppDefault` /
/// `{"Explicit":[…]}`). Chirp always uses `UserPreferred` (the active account's
/// published kind:10007 search relays).
enum IntentTextTargets: Encodable {
    case userPreferred

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .userPreferred: try c.encode("UserPreferred")
        }
    }
}

/// The `InputIntentRequest` wire shape: the raw input, the app's requested
/// scopes allow-list, and the free-text target choice.
struct IntentRequest: Encodable {
    let input: String
    let scopes: [IntentScope]
    let textTargets: IntentTextTargets

    private enum CodingKeys: String, CodingKey {
        case input
        case scopes
        case textTargets = "text_targets"
    }

    func jsonString() -> String? {
        guard let data = try? JSONEncoder().encode(self) else { return nil }
        return String(data: data, encoding: .utf8)
    }
}

// ── Dispatch outcome ─────────────────────────────────────────────────────────

/// The typed result of `nmp_app_intent_dispatch`: either a dispatched candidate
/// (the resolver acted on it through an NMP seam) or a rejection. The host reads
/// this to drive navigation / show a notice — it never re-decides.
enum IntentDispatchOutcome: Equatable {
    case dispatched(IntentTarget)
    case rejection(IntentRejection)

    /// Decode the `{"ok":true,"dispatched":<candidate>}` /
    /// `{"ok":true,"rejection":<rejection>}` envelope.
    static func decode(json: String) -> IntentDispatchOutcome? {
        guard let data = json.data(using: .utf8),
              let env = try? JSONDecoder().decode(Envelope.self, from: data),
              env.ok else { return nil }
        if let candidate = env.dispatched {
            return .dispatched(candidate.target)
        }
        if let rejection = env.rejection {
            return .rejection(rejection)
        }
        return nil
    }

    private struct Envelope: Decodable {
        let ok: Bool
        let dispatched: Candidate?
        let rejection: IntentRejection?
    }

    private struct Candidate: Decodable {
        let target: IntentTarget
    }
}

/// The candidate target the resolver chose. Matches the externally-tagged
/// `InputIntentTarget` enum (`{"DirectRef":{"uri":"…"}}`, etc.).
enum IntentTarget: Decodable, Equatable {
    case directRef(uri: String)
    case nip05(identifier: String)
    case relayURL(url: String)
    case textQuery
    case registered

    private enum CodingKeys: String, CodingKey {
        case DirectRef, Nip05, RelayUrl, TextQuery, Registered
    }

    private struct DirectRefBody: Decodable { let uri: String }
    private struct Nip05Body: Decodable { let identifier: String }
    private struct RelayURLBody: Decodable { let url: String }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if let body = try c.decodeIfPresent(DirectRefBody.self, forKey: .DirectRef) {
            self = .directRef(uri: body.uri)
        } else if let body = try c.decodeIfPresent(Nip05Body.self, forKey: .Nip05) {
            self = .nip05(identifier: body.identifier)
        } else if let body = try c.decodeIfPresent(RelayURLBody.self, forKey: .RelayUrl) {
            self = .relayURL(url: body.url)
        } else if c.contains(.TextQuery) {
            self = .textQuery
        } else if c.contains(.Registered) {
            self = .registered
        } else {
            throw DecodingError.dataCorrupted(
                .init(codingPath: decoder.codingPath, debugDescription: "unknown intent target")
            )
        }
    }
}

/// Why an input was refused. Matches `InputIntentRejection`: `SecretLike` and
/// `Unparseable` are bare strings; `UnregisteredScope` / `DisallowedScope` carry
/// the offending scope object.
enum IntentRejection: Decodable, Equatable {
    case secretLike
    case unparseable
    case unregisteredScope
    case disallowedScope

    private enum ObjectKeys: String, CodingKey {
        case UnregisteredScope, DisallowedScope
    }

    init(from decoder: Decoder) throws {
        if let single = try? decoder.singleValueContainer(), let s = try? single.decode(String.self) {
            switch s {
            case "SecretLike": self = .secretLike; return
            case "Unparseable": self = .unparseable; return
            default: break
            }
        }
        let c = try decoder.container(keyedBy: ObjectKeys.self)
        if c.contains(.UnregisteredScope) {
            self = .unregisteredScope
        } else if c.contains(.DisallowedScope) {
            self = .disallowedScope
        } else {
            self = .unparseable
        }
    }
}

// ── Decoded ref target (nmp_nip21_decode_uri) ────────────────────────────────

/// The typed decode of a `DirectRef` URI, mirroring the `nmp_nip21_decode_uri`
/// envelope `{"ok":true,"target":"profile"|"event"|"address",…}`. Maps onto a
/// `ChirpRoute` in the controller (the only navigation step) — Swift never
/// parses bech32 itself.
enum DecodedRefTarget: Equatable {
    case profile(pubkey: String)
    case event(eventID: String)
    case address(pubkey: String)

    static func decode(json: String) -> DecodedRefTarget? {
        guard let data = json.data(using: .utf8),
              let env = try? JSONDecoder().decode(Envelope.self, from: data),
              env.ok else { return nil }
        switch env.target {
        case "profile":
            guard let pk = env.pubkey else { return nil }
            return .profile(pubkey: pk)
        case "event":
            guard let id = env.eventID else { return nil }
            return .event(eventID: id)
        case "address":
            guard let pk = env.pubkey else { return nil }
            return .address(pubkey: pk)
        default:
            return nil
        }
    }

    private struct Envelope: Decodable {
        let ok: Bool
        let target: String?
        let pubkey: String?
        let eventID: String?

        private enum CodingKeys: String, CodingKey {
            case ok, target, pubkey
            case eventID = "event_id"
        }
    }
}
