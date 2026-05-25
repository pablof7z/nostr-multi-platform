import SwiftUI

/// Swift Codable mirror of the Rust `nmp_content::wire::ContentTreeWire`.
///
/// The Rust type is a flat *arena*: `nodes: [WireNode]` plus `roots: [UInt32]`.
/// Every recursive parent→child edge in the source tree is a `[UInt32]` of
/// indices into `nodes`. See `crates/nmp-content/src/wire/mod.rs` for the
/// canonical definition.
///
/// This file is shipped as a registry component so apps that install
/// `swiftui/content-core` get a complete, drift-resistant mirror without
/// hand-rolling Decodables.
public struct ContentTreeWire: Decodable, Equatable, Sendable {
    public let nodes: [NostrWireNode]
    public let roots: [UInt32]
    public let mode: String?

    public init(nodes: [NostrWireNode], roots: [UInt32], mode: String? = nil) {
        self.nodes = nodes
        self.roots = roots
        self.mode = mode
    }

    /// Bounds-checked arena lookup.
    public func node(at index: UInt32) -> NostrWireNode? {
        let i = Int(index)
        guard i >= 0, i < nodes.count else { return nil }
        return nodes[i]
    }
}

/// Typed mirror of the Rust `nmp_content::MediaKind` enum. Raw values match the
/// PascalCase Rust serde representation (the Rust enum has no `rename_all`).
public enum NostrMediaKind: String, Decodable, Equatable, Sendable {
    case image = "Image"
    case video = "Video"
    case audio = "Audio"
}

/// NIP-21 entity discriminator on the wire.
public enum NostrWireUriKind: String, Decodable, Equatable, Sendable {
    case profile
    case event
    case address
}

/// Why a `placeholder` wire node was emitted.
public enum NostrWirePlaceholderReason: String, Decodable, Equatable, Sendable {
    case depthLimit = "depth_limit"
    case unresolvedUri = "unresolved_uri"
}

/// Reserved payment segment (`WireNode::Invoice`).
public enum NostrWireInvoice: Decodable, Equatable, Sendable {
    case bolt11(String)
    case bolt12(String)
    case cashu(String)

    private enum CodingKeys: String, CodingKey {
        case bolt11 = "Bolt11"
        case bolt12 = "Bolt12"
        case cashu = "Cashu"
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let value = try container.decodeIfPresent(String.self, forKey: .bolt11) {
            self = .bolt11(value)
        } else if let value = try container.decodeIfPresent(String.self, forKey: .bolt12) {
            self = .bolt12(value)
        } else if let value = try container.decodeIfPresent(String.self, forKey: .cashu) {
            self = .cashu(value)
        } else {
            throw DecodingError.dataCorrupted(
                .init(codingPath: decoder.codingPath, debugDescription: "unknown invoice kind")
            )
        }
    }
}

/// Flattened, Codable projection of `nmp_core::nip21::NostrUri`. `uri` is the
/// round-trippable canonical `nostr:` URI; `primaryId` is the hex pubkey for
/// profiles, event id for events, or author pubkey for addresses.
public struct NostrWireUri: Decodable, Equatable, Sendable {
    public let uri: String
    public let kind: NostrWireUriKind
    public let primaryId: String
    public let relays: [String]
    public let author: String?
    public let eventKind: UInt32?

    private enum CodingKeys: String, CodingKey {
        case uri, kind, relays, author
        case primaryId = "primary_id"
        case eventKind = "event_kind"
    }

    public init(
        uri: String,
        kind: NostrWireUriKind,
        primaryId: String,
        relays: [String] = [],
        author: String? = nil,
        eventKind: UInt32? = nil
    ) {
        self.uri = uri
        self.kind = kind
        self.primaryId = primaryId
        self.relays = relays
        self.author = author
        self.eventKind = eventKind
    }
}

/// One node in the `ContentTreeWire` arena. Covers every variant of the Rust
/// `WireNode` enum.
public enum NostrWireNode: Decodable, Equatable, Sendable {
    case text(String)
    case mention(NostrWireUri)
    case eventRef(NostrWireUri)
    case hashtag(String)
    case url(String)
    case media(urls: [String], kind: NostrMediaKind)
    case emoji(shortcode: String, url: String?)
    case invoice(NostrWireInvoice)
    case heading(level: UInt8, children: [UInt32])
    case paragraph(children: [UInt32])
    case blockQuote(children: [UInt32])
    case codeBlock(info: String?, body: String)
    case list(orderedStart: UInt64?, items: [[UInt32]])
    case rule
    case emphasis(children: [UInt32])
    case strong(children: [UInt32])
    case inlineCode(String)
    case link(children: [UInt32], href: String?)
    case image(alt: String, title: String?, src: String?)
    case softBreak
    case hardBreak
    case placeholder(reason: NostrWirePlaceholderReason)

    private enum CodingKeys: String, CodingKey {
        case kind, text, uri, tag, url, urls
        case mediaKind = "media_kind"
        case shortcode, level, children, code, info, body, href
        case orderedStart = "ordered_start"
        case items, alt, title, src, reason, invoice
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let kind = try container.decode(String.self, forKey: .kind)
        switch kind {
        case "text":
            self = .text(try container.decode(String.self, forKey: .text))
        case "mention":
            self = .mention(try container.decode(NostrWireUri.self, forKey: .uri))
        case "event_ref":
            self = .eventRef(try container.decode(NostrWireUri.self, forKey: .uri))
        case "hashtag":
            self = .hashtag(try container.decode(String.self, forKey: .tag))
        case "url":
            self = .url(try container.decode(String.self, forKey: .url))
        case "media":
            self = .media(
                urls: try container.decode([String].self, forKey: .urls),
                kind: try container.decode(NostrMediaKind.self, forKey: .mediaKind)
            )
        case "emoji":
            self = .emoji(
                shortcode: try container.decode(String.self, forKey: .shortcode),
                url: try container.decodeIfPresent(String.self, forKey: .url)
            )
        case "invoice":
            self = .invoice(try container.decode(NostrWireInvoice.self, forKey: .invoice))
        case "heading":
            self = .heading(
                level: try container.decode(UInt8.self, forKey: .level),
                children: try container.decode([UInt32].self, forKey: .children)
            )
        case "paragraph":
            self = .paragraph(children: try container.decode([UInt32].self, forKey: .children))
        case "block_quote":
            self = .blockQuote(children: try container.decode([UInt32].self, forKey: .children))
        case "code_block":
            self = .codeBlock(
                info: try container.decodeIfPresent(String.self, forKey: .info),
                body: try container.decode(String.self, forKey: .body)
            )
        case "list":
            self = .list(
                orderedStart: try container.decodeIfPresent(UInt64.self, forKey: .orderedStart),
                items: try container.decode([[UInt32]].self, forKey: .items)
            )
        case "rule":
            self = .rule
        case "emphasis":
            self = .emphasis(children: try container.decode([UInt32].self, forKey: .children))
        case "strong":
            self = .strong(children: try container.decode([UInt32].self, forKey: .children))
        case "inline_code":
            self = .inlineCode(try container.decode(String.self, forKey: .code))
        case "link":
            self = .link(
                children: try container.decode([UInt32].self, forKey: .children),
                href: try container.decodeIfPresent(String.self, forKey: .href)
            )
        case "image":
            self = .image(
                alt: try container.decode(String.self, forKey: .alt),
                title: try container.decodeIfPresent(String.self, forKey: .title),
                src: try container.decodeIfPresent(String.self, forKey: .src)
            )
        case "soft_break":
            self = .softBreak
        case "hard_break":
            self = .hardBreak
        case "placeholder":
            self = .placeholder(
                reason: try container.decode(NostrWirePlaceholderReason.self, forKey: .reason)
            )
        default:
            // Forward-compat: any unknown kind collapses to a depth_limit
            // placeholder rather than failing the whole decode.
            self = .placeholder(reason: .depthLimit)
        }
    }
}

/// Deterministic identicon color for a hex pubkey. Apps that don't supply an
/// avatar URL can render this as a circle background. Ported from Chirp's
/// djb2-based palette so installed apps stay visually consistent.
public enum NostrIdenticon {
    /// Returns a stable `Color` derived from a hex pubkey (or any string).
    /// Uses the djb2 hash mapped to HSL with fixed S/L for legibility.
    public static func color(forPubkey pubkey: String) -> Color {
        let hue = Double(djb2(pubkey) % 360) / 360.0
        return Color(hue: hue, saturation: 0.55, brightness: 0.75)
    }

    /// Two-character monogram derived from a hex pubkey (first two chars,
    /// uppercased). Apps with kind:0 profile data should provide their own.
    public static func initials(forPubkey pubkey: String) -> String {
        let trimmed = pubkey.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return "?" }
        return String(trimmed.prefix(2)).uppercased()
    }

    private static func djb2(_ value: String) -> UInt32 {
        var hash: UInt32 = 5381
        for byte in value.utf8 {
            hash = (hash &* 33) &+ UInt32(byte)
        }
        return hash
    }
}
