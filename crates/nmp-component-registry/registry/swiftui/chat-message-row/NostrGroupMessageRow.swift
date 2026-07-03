import SwiftUI

public struct NostrGroupMessageRow: View {
    public var message: NostrGroupChatMessageWire
    public var onReplyTap: (String) -> Void

    public init(
        message: NostrGroupChatMessageWire,
        onReplyTap: @escaping (String) -> Void = { _ in }
    ) {
        self.message = message
        self.onReplyTap = onReplyTap
    }

    public var body: some View {
        HStack(alignment: .top, spacing: 8) {
            if !message.isOutgoing {
                NostrAvatar(
                    pubkey: message.authorPubkey,
                    size: 32,
                    consumerID: "chat.message.\(message.id).avatar"
                )
            }

            VStack(alignment: message.isOutgoing ? .trailing : .leading, spacing: 4) {
                if !message.isOutgoing {
                    HStack(spacing: 6) {
                        NostrProfileName(
                            pubkey: message.authorPubkey,
                            font: .caption.weight(.semibold),
                            color: .secondary,
                            consumerID: "chat.message.\(message.id).name"
                        )
                        Text(message.createdAtLabel)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                if let replyPreview = message.replyPreview, !replyPreview.isEmpty {
                    Button {
                        onReplyTap(message.id)
                    } label: {
                        Text(replyPreview)
                            .font(.caption)
                            .lineLimit(2)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 5)
                            .background(.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 6))
                    }
                    .buttonStyle(.plain)
                }

                Text(message.content)
                    .font(.body)
                    .foregroundStyle(message.isOutgoing ? .white : .primary)
                    .textSelection(.enabled)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                    .background(
                        message.isOutgoing ? Color.accentColor : Color.secondary.opacity(0.10),
                        in: RoundedRectangle(cornerRadius: 8)
                    )

                if !message.reactions.isEmpty {
                    HStack(spacing: 4) {
                        ForEach(message.reactions) { reaction in
                            Text("\(reaction.emoji) \(reaction.count)")
                                .font(.caption2.weight(.medium))
                                .padding(.horizontal, 7)
                                .padding(.vertical, 3)
                                .background(.secondary.opacity(0.10), in: Capsule())
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: message.isOutgoing ? .trailing : .leading)
        }
        .accessibilityElement(children: .combine)
    }
}
