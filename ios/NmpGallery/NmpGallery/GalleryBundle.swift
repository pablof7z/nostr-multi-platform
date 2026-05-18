import Foundation

// Swift Codable mirror of `crates/nmp-content-fixtures/src/dto.rs`.
//
// PROJECTION-GAP NOTE: `nmp_content::Segment` / `ContentTree` /
// `MarkdownNode` are deliberately non-serde with no FFI projection (T93).
// STAGE 2 projects them to a serde JSON mirror; this file is the Swift
// decode side of that mirror. The `type` discriminator matches serde's
// `#[serde(tag = "type", rename_all = "camelCase")]`.

struct GalleryBundle: Decodable {
    let version: Int
    let scenarios: [Scenario]
}

struct Scenario: Decodable, Identifiable {
    let id: String
    let category: String
    let title: String
    let exercises: String
    let events: [SignedEventJson]
    let rendered: ContentTreeDto
    let embeds: [String: EmbedEntry]
}

struct SignedEventJson: Decodable {
    let id: String
    let pubkey: String
    let createdAt: UInt64
    let kind: UInt32
    let tags: [[String]]
    let content: String
    let sig: String

    enum CodingKeys: String, CodingKey {
        case id, pubkey, kind, tags, content, sig
        case createdAt = "created_at"
    }
}

struct ContentTreeDto: Decodable {
    let mode: String
    let segments: [SegmentDto]
}

indirect enum SegmentDto: Decodable {
    case text(String)
    case mention(uri: String, kind: String, pubkey: String)
    case eventRef(uri: String, kind: String, id: String)
    case hashtag(String)
    case url(String)
    case media(mediaKind: String, urls: [String])
    case emoji(shortcode: String, url: String?)
    case invoice(invoiceKind: String, value: String)
    case markdownBlock(MarkdownNodeDto)
    case unknown(String)

    enum CodingKeys: String, CodingKey {
        case type, text, uri, kind, pubkey, id, tag, url
        case urls, shortcode, value, node
        // serde rename_all=camelCase renames VARIANT names only; struct
        // FIELDS stay snake_case in the emitted JSON.
        case mediaKind = "media_kind"
        case invoiceKind = "invoice_kind"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "text":
            self = .text(try c.decode(String.self, forKey: .text))
        case "mention":
            self = .mention(
                uri: try c.decode(String.self, forKey: .uri),
                kind: try c.decode(String.self, forKey: .kind),
                pubkey: try c.decode(String.self, forKey: .pubkey))
        case "eventRef":
            self = .eventRef(
                uri: try c.decode(String.self, forKey: .uri),
                kind: try c.decode(String.self, forKey: .kind),
                id: try c.decode(String.self, forKey: .id))
        case "hashtag":
            self = .hashtag(try c.decode(String.self, forKey: .tag))
        case "url":
            self = .url(try c.decode(String.self, forKey: .url))
        case "media":
            self = .media(
                mediaKind: try c.decode(String.self, forKey: .mediaKind),
                urls: try c.decode([String].self, forKey: .urls))
        case "emoji":
            self = .emoji(
                shortcode: try c.decode(String.self, forKey: .shortcode),
                url: try c.decodeIfPresent(String.self, forKey: .url))
        case "invoice":
            self = .invoice(
                invoiceKind: try c.decode(String.self, forKey: .invoiceKind),
                value: try c.decode(String.self, forKey: .value))
        case "markdownBlock":
            self = .markdownBlock(
                try c.decode(MarkdownNodeDto.self, forKey: .node))
        default:
            self = .unknown(type)
        }
    }
}

indirect enum MarkdownNodeDto: Decodable {
    case heading(level: Int, inlines: [MarkdownInlineDto])
    case paragraph([MarkdownInlineDto])
    case blockQuote([MarkdownNodeDto])
    case codeBlock(info: String?, body: String)
    case list(orderedStart: UInt64?, items: [[MarkdownNodeDto]])
    case rule
    case unknown(String)

    enum CodingKeys: String, CodingKey {
        case type, level, inlines, blocks, info, body, items
        case orderedStart = "ordered_start"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "heading":
            self = .heading(
                level: try c.decode(Int.self, forKey: .level),
                inlines: try c.decode(
                    [MarkdownInlineDto].self, forKey: .inlines))
        case "paragraph":
            self = .paragraph(try c.decode(
                [MarkdownInlineDto].self, forKey: .inlines))
        case "blockQuote":
            self = .blockQuote(try c.decode(
                [MarkdownNodeDto].self, forKey: .blocks))
        case "codeBlock":
            self = .codeBlock(
                info: try c.decodeIfPresent(String.self, forKey: .info),
                body: try c.decode(String.self, forKey: .body))
        case "list":
            self = .list(
                orderedStart: try c.decodeIfPresent(
                    UInt64.self, forKey: .orderedStart),
                items: try c.decode(
                    [[MarkdownNodeDto]].self, forKey: .items))
        case "rule":
            self = .rule
        default:
            self = .unknown(type)
        }
    }
}

indirect enum MarkdownInlineDto: Decodable {
    case inline(SegmentDto)
    case emphasis([MarkdownInlineDto])
    case strong([MarkdownInlineDto])
    case code(String)
    case link(label: [MarkdownInlineDto], href: String?)
    case image(alt: String, title: String?, src: String?)
    case softBreak
    case hardBreak
    case unknown(String)

    enum CodingKeys: String, CodingKey {
        case type, segment, children, text, label, href
        case alt, title, src
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "inline":
            self = .inline(try c.decode(SegmentDto.self, forKey: .segment))
        case "emphasis":
            self = .emphasis(try c.decode(
                [MarkdownInlineDto].self, forKey: .children))
        case "strong":
            self = .strong(try c.decode(
                [MarkdownInlineDto].self, forKey: .children))
        case "code":
            self = .code(try c.decode(String.self, forKey: .text))
        case "link":
            self = .link(
                label: try c.decode(
                    [MarkdownInlineDto].self, forKey: .label),
                href: try c.decodeIfPresent(String.self, forKey: .href))
        case "image":
            self = .image(
                alt: try c.decode(String.self, forKey: .alt),
                title: try c.decodeIfPresent(String.self, forKey: .title),
                src: try c.decodeIfPresent(String.self, forKey: .src))
        case "softBreak":
            self = .softBreak
        case "hardBreak":
            self = .hardBreak
        default:
            self = .unknown(type)
        }
    }
}

// Embed DTOs (EmbedEntry / ArticleHeaderDto / ListDto / ListRowDto) and
// the BundleLoader live in GalleryEmbedDto.swift to keep this file under
// the 300-LOC budget.
