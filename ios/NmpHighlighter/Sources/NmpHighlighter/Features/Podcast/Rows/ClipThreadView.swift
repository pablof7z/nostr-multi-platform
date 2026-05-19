import SwiftUI

/// Thread display + reply composer rendered inside an expanded `MemberClipRow`.
struct ClipThreadView: View {
    @Environment(HighlighterStore.self) private var app

    let clipEventId: String

    @State private var replyText: String = ""
    @State private var isSending: Bool = false
    @State private var sendError: String? = nil

    private var comments: [CommentRecord]? {
        app.podcastPlayer.comments[clipEventId]
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Divider()
                .padding(.horizontal, 16)

            if comments == nil {
                HStack {
                    Spacer()
                    ProgressView()
                    Spacer()
                }
                .padding(.vertical, 16)
            } else if let list = comments, list.isEmpty {
                Text("No replies yet")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
            } else if let list = comments {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(list.reversed(), id: \.eventId) { comment in
                        CommentRowView(comment: comment)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
            }

            Divider()
                .padding(.horizontal, 16)

            HStack(spacing: 10) {
                TextField("Reply...", text: $replyText)
                    .font(.subheadline)
                    .tint(Color.highlighterAccent)

                if isSending {
                    ProgressView()
                        .scaleEffect(0.8)
                } else {
                    Button("Send") {
                        send()
                    }
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(replyText.trimmingCharacters(in: .whitespaces).isEmpty
                        ? Color.secondary
                        : Color.highlighterAccent)
                    .disabled(replyText.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)

            if let error = sendError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 8)
            }
        }
    }

    private func send() {
        let text = replyText.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty, !isSending else { return }
        isSending = true
        sendError = nil
        let id = clipEventId
        Task {
            do {
                let record = try await app.safeCore.publishComment(
                    rootTagName: "e",
                    rootTagValue: id,
                    rootKind: 9802,
                    content: text
                )
                var existing = app.podcastPlayer.comments[id] ?? []
                existing.append(record)
                app.podcastPlayer.comments[id] = existing
                replyText = ""
            } catch {
                sendError = error.localizedDescription
            }
            isSending = false
        }
    }
}

private struct CommentRowView: View {
    @Environment(HighlighterStore.self) private var app
    let comment: CommentRecord

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            AuthorAvatar(
                pubkey: comment.pubkey,
                pictureURL: app.profileCache[comment.pubkey]?.picture ?? "",
                displayInitial: initial,
                size: 26
            )

            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(name)
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    if let t = relativeTime {
                        Text("·").foregroundStyle(.secondary)
                        Text(t)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                Text(comment.body)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .task(id: comment.pubkey) {
            await app.requestProfile(pubkeyHex: comment.pubkey)
        }
    }

    private var name: String {
        let profile = app.profileCache[comment.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(comment.pubkey.prefix(10))
    }

    private var initial: String {
        name.first.map { String($0).uppercased() } ?? ""
    }

    private var relativeTime: String? {
        guard let s = comment.createdAt, s > 0 else { return nil }
        let date = Date(timeIntervalSince1970: TimeInterval(s))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        formatter.dateTimeStyle = .numeric
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}
