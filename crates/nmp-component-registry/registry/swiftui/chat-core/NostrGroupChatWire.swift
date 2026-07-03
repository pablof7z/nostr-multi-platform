import Foundation

public struct NostrGroupChatReactionWire: Identifiable, Equatable, Sendable {
    public var emoji: String
    public var count: Int

    public var id: String { emoji }

    public init(emoji: String, count: Int) {
        self.emoji = emoji
        self.count = count
    }
}

public struct NostrGroupChatMessageWire: Identifiable, Equatable, Sendable {
    public var id: String
    public var authorPubkey: String
    public var content: String
    public var createdAtLabel: String
    public var replyPreview: String?
    public var reactions: [NostrGroupChatReactionWire]
    public var isOutgoing: Bool

    public init(
        id: String,
        authorPubkey: String,
        content: String,
        createdAtLabel: String,
        replyPreview: String? = nil,
        reactions: [NostrGroupChatReactionWire] = [],
        isOutgoing: Bool = false
    ) {
        self.id = id
        self.authorPubkey = authorPubkey
        self.content = content
        self.createdAtLabel = createdAtLabel
        self.replyPreview = replyPreview
        self.reactions = reactions
        self.isOutgoing = isOutgoing
    }
}

public struct NostrGroupChatParticipantWire: Identifiable, Equatable, Sendable {
    public var pubkey: String
    public var roleLabel: String?
    public var statusLabel: String?

    public var id: String { pubkey }

    public init(pubkey: String, roleLabel: String? = nil, statusLabel: String? = nil) {
        self.pubkey = pubkey
        self.roleLabel = roleLabel
        self.statusLabel = statusLabel
    }
}
