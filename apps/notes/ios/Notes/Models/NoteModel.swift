import Foundation

/// A single kind:1 note projected from raw NIP-01 signed-event JSON. The
/// spike decodes only the fields the timeline needs — no NIP-23/27 parsing
/// — to keep the proof minimal.
struct NoteModel: Identifiable, Hashable {
    let id: String
    let pubkey: String
    let content: String
    let createdAt: Date

    /// Parse `{id,pubkey,created_at,kind,tags,content,sig}`. Returns nil
    /// for malformed input.
    static func parse(_ json: String) -> NoteModel? {
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let id = obj["id"] as? String,
              let pubkey = obj["pubkey"] as? String,
              let createdAt = obj["created_at"] as? TimeInterval,
              let content = obj["content"] as? String else { return nil }
        return NoteModel(id: id, pubkey: pubkey, content: content,
                         createdAt: Date(timeIntervalSince1970: createdAt))
    }
}
