import SwiftUI

public struct NostrGroupRosterList: View {
    public var participants: [NostrGroupChatParticipantWire]
    public var onSelectParticipant: (String) -> Void

    public init(
        participants: [NostrGroupChatParticipantWire],
        onSelectParticipant: @escaping (String) -> Void = { _ in }
    ) {
        self.participants = participants
        self.onSelectParticipant = onSelectParticipant
    }

    public var body: some View {
        LazyVStack(alignment: .leading, spacing: 10) {
            ForEach(participants) { participant in
                Button {
                    onSelectParticipant(participant.pubkey)
                } label: {
                    HStack(spacing: 10) {
                        NostrAvatar(
                            pubkey: participant.pubkey,
                            size: 36,
                            consumerID: "chat.roster.\(participant.pubkey).avatar"
                        )

                        VStack(alignment: .leading, spacing: 2) {
                            NostrProfileName(
                                pubkey: participant.pubkey,
                                font: .subheadline.weight(.semibold),
                                consumerID: "chat.roster.\(participant.pubkey).name"
                            )
                            HStack(spacing: 6) {
                                if let role = participant.roleLabel, !role.isEmpty {
                                    Text(role)
                                }
                                if let status = participant.statusLabel, !status.isEmpty {
                                    Text(status)
                                }
                            }
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        }

                        Spacer(minLength: 0)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }
}
