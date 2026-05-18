import Foundation

// Embed-resolution DTOs + bundle loader. Split from GalleryBundle.swift to
// keep each file under the 300-LOC budget. Mirrors the embed half of
// `crates/nmp-content-fixtures/src/dto.rs`.

struct EmbedEntry: Decodable {
    let resolvedKind: UInt32
    let profileName: String?
    let profilePicture: String?
    let event: SignedEventJson?
    let rendered: ContentTreeDto?
    let collapsed: Bool
    let collapseReason: String?
    let article: ArticleHeaderDto?
    let list: ListDto?

    enum CodingKeys: String, CodingKey {
        case event, rendered, collapsed, article, list
        case resolvedKind = "resolved_kind"
        case profileName = "profile_name"
        case profilePicture = "profile_picture"
        case collapseReason = "collapse_reason"
    }
}

struct ArticleHeaderDto: Decodable {
    let title: String?
    let summary: String?
    let author: String
    let dTag: String

    enum CodingKeys: String, CodingKey {
        case title, summary, author
        case dTag = "d_tag"
    }
}

struct ListDto: Decodable {
    let title: String?
    let rows: [ListRowDto]
}

enum ListRowDto: Decodable {
    case profile(pubkey: String, name: String?, picture: String?)
    case event(id: String)
    case address(coord: String)
    case hashtag(tag: String)
    case relay(url: String, read: Bool, write: Bool)
    case unknown(String)

    enum CodingKeys: String, CodingKey {
        case type, pubkey, name, picture, id, coord, tag
        case url, read, write
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "profile":
            self = .profile(
                pubkey: try c.decode(String.self, forKey: .pubkey),
                name: try c.decodeIfPresent(String.self, forKey: .name),
                picture: try c.decodeIfPresent(
                    String.self, forKey: .picture))
        case "event":
            self = .event(id: try c.decode(String.self, forKey: .id))
        case "address":
            self = .address(coord: try c.decode(
                String.self, forKey: .coord))
        case "hashtag":
            self = .hashtag(tag: try c.decode(String.self, forKey: .tag))
        case "relay":
            self = .relay(
                url: try c.decode(String.self, forKey: .url),
                read: try c.decode(Bool.self, forKey: .read),
                write: try c.decode(Bool.self, forKey: .write))
        default:
            self = .unknown(type)
        }
    }
}

struct BundleLoadError: Error {
    let message: String
}

enum BundleLoader {
    /// Decode the committed offline bundle shipped as an app resource.
    static func load() -> Result<GalleryBundle, BundleLoadError> {
        guard let url = Bundle.main.url(
            forResource: "content-gallery-bundle",
            withExtension: "json")
        else {
            return .failure(BundleLoadError(
                message: "content-gallery-bundle.json not in app bundle"))
        }
        do {
            let data = try Data(contentsOf: url)
            let bundle = try JSONDecoder().decode(
                GalleryBundle.self, from: data)
            return .success(bundle)
        } catch {
            return .failure(BundleLoadError(
                message: "decode failed: \(error)"))
        }
    }
}
