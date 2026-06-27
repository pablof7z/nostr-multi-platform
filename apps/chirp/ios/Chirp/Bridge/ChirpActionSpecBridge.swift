import Foundation

enum InterestScope: UInt32 {
    case activeAccount = 0
    case global = 1
}

struct ChirpReplyTarget: Codable, Equatable, Identifiable {
    let eventID: String
    let authorPubkey: String
    let createdAt: UInt64
    let content: String

    var id: String { eventID }

    init(eventID: String, authorPubkey: String, createdAt: UInt64 = 0, content: String = "") {
        self.eventID = eventID
        self.authorPubkey = authorPubkey
        self.createdAt = createdAt
        self.content = content
    }

    init(row: NoteRowModel) {
        self.init(
            eventID: row.id,
            authorPubkey: row.authorPubkey,
            createdAt: row.createdAt,
            content: row.content
        )
    }

    private enum CodingKeys: String, CodingKey {
        case eventID = "event_id"
        case authorPubkey = "author_pubkey"
        case createdAt = "created_at"
        case content
    }
}

struct ChirpActionIntent: Encodable, Equatable {
    let type: String
    let content: String?
    let replyTo: ChirpReplyTarget?
    let replyToEventID: String?
    let dmReplyTo: String?
    let name: String?
    let about: String?
    let picture: String?
    let eventID: String?
    let authorPubkey: String?
    let reaction: String?
    let pubkey: String?
    let targetEventID: String?
    let recipientPubkey: String?
    let amountMsats: UInt64?
    let lnurl: String?
    let comment: String?
    static func publishNote(content: String, replyTo: ChirpReplyTarget?) -> Self {
        Self(type: "publish_note", content: content, replyTo: replyTo)
    }

    static func publishProfile(name: String, about: String, picture: String) -> Self {
        Self(type: "publish_profile", name: name, about: about, picture: picture)
    }

    static func repost(eventID: String, authorPubkey: String) -> Self {
        Self(type: "repost", eventID: eventID, authorPubkey: authorPubkey)
    }

    static func react(eventID: String, reaction: String) -> Self {
        Self(type: "react", eventID: eventID, reaction: reaction)
    }

    static func follow(pubkey: String) -> Self {
        Self(type: "follow", pubkey: pubkey)
    }

    static func unfollow(pubkey: String) -> Self {
        Self(type: "unfollow", pubkey: pubkey)
    }

    static func zap(
        targetEventID: String,
        recipientPubkey: String,
        amountMsats: UInt64,
        lnurl: String,
        comment: String?
    ) -> Self {
        Self(
            type: "zap",
            targetEventID: targetEventID,
            recipientPubkey: recipientPubkey,
            amountMsats: amountMsats,
            lnurl: lnurl,
            comment: comment
        )
    }

    static func sendDm(recipientPubkey: String, content: String, replyTo: String?) -> Self {
        Self(
            type: "send_dm",
            content: content,
            dmReplyTo: replyTo,
            recipientPubkey: recipientPubkey
        )
    }

    private init(
        type: String,
        content: String? = nil,
        replyTo: ChirpReplyTarget? = nil,
        replyToEventID: String? = nil,
        dmReplyTo: String? = nil,
        name: String? = nil,
        about: String? = nil,
        picture: String? = nil,
        eventID: String? = nil,
        authorPubkey: String? = nil,
        reaction: String? = nil,
        pubkey: String? = nil,
        targetEventID: String? = nil,
        recipientPubkey: String? = nil,
        amountMsats: UInt64? = nil,
        lnurl: String? = nil,
        comment: String? = nil
    ) {
        self.type = type
        self.content = content
        self.replyTo = replyTo
        self.replyToEventID = replyToEventID
        self.dmReplyTo = dmReplyTo
        self.name = name
        self.about = about
        self.picture = picture
        self.eventID = eventID
        self.authorPubkey = authorPubkey
        self.reaction = reaction
        self.pubkey = pubkey
        self.targetEventID = targetEventID
        self.recipientPubkey = recipientPubkey
        self.amountMsats = amountMsats
        self.lnurl = lnurl
        self.comment = comment
    }

    private enum CodingKeys: String, CodingKey {
        case type, content, name, about, picture, reaction, pubkey, lnurl, comment
        case replyTo = "reply_to"
        case replyToEventID = "reply_to_event_id"
        case eventID = "event_id"
        case authorPubkey = "author_pubkey"
        case targetEventID = "target_event_id"
        case recipientPubkey = "recipient_pubkey"
        case amountMsats = "amount_msats"
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(type, forKey: .type)
        try c.encodeIfPresent(content, forKey: .content)
        try c.encodeIfPresent(replyTo, forKey: .replyTo)
        try c.encodeIfPresent(replyToEventID, forKey: .replyToEventID)
        try c.encodeIfPresent(dmReplyTo, forKey: .replyTo)
        try c.encodeIfPresent(name, forKey: .name)
        try c.encodeIfPresent(about, forKey: .about)
        try c.encodeIfPresent(picture, forKey: .picture)
        try c.encodeIfPresent(eventID, forKey: .eventID)
        try c.encodeIfPresent(authorPubkey, forKey: .authorPubkey)
        try c.encodeIfPresent(reaction, forKey: .reaction)
        try c.encodeIfPresent(pubkey, forKey: .pubkey)
        try c.encodeIfPresent(targetEventID, forKey: .targetEventID)
        try c.encodeIfPresent(recipientPubkey, forKey: .recipientPubkey)
        try c.encodeIfPresent(amountMsats, forKey: .amountMsats)
        try c.encodeIfPresent(lnurl, forKey: .lnurl)
        try c.encodeIfPresent(comment, forKey: .comment)
    }
}

struct ChirpActionSpecEnvelope: Decodable {
    let namespace: String?
    let bodyJson: String?
    let error: String?

    private enum CodingKeys: String, CodingKey {
        case namespace
        case bodyJson = "body_json"
        case error
    }
}
