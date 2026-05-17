import SwiftUI

struct ProfileCardView: View {
    let profile: ProfileCard

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            RemoteAvatar(
                url: profile.pictureUrl,
                initials: profile.avatarInitials,
                color: profile.avatarColor,
                source: profile.source,
                size: 52
            )
            VStack(alignment: .leading, spacing: 5) {
                Text(profile.display)
                    .font(.headline)
                    .lineLimit(1)
                if !profile.nip05.isEmpty {
                    Text(profile.nip05)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text(profile.about)
                    .font(.footnote)
                    .lineLimit(3)
                Text(profile.npub)
                    .font(.caption2.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 6)
    }
}

struct ProfileDetailView: View {
    @EnvironmentObject private var model: KernelModel
    let pubkey: String

    private var view: AuthorViewPayload? {
        if model.authorView?.pubkey == pubkey {
            return model.authorView
        }
        return model.cachedAuthorView(pubkey: pubkey)
    }

    var body: some View {
        List {
            Section {
                ProfileCardView(profile: view?.profile ?? ProfileCard.placeholder(pubkey: pubkey))
                HStack {
                    Text("State")
                        .foregroundStyle(.secondary)
                    Spacer()
                    Text(view?.state ?? "opening")
                        .font(.caption.monospacedDigit())
                }
                .font(.caption)
                HStack {
                    Text("Notes")
                        .foregroundStyle(.secondary)
                    Spacer()
                    Text("\(view?.noteCount ?? 0)")
                        .font(.caption.monospacedDigit())
                        .accessibilityIdentifier("profile-notes-count-value")
                }
                .font(.caption)
            }

            Section("Notes") {
                if let items = view?.items, !items.isEmpty {
                    ForEach(items) { note in
                        NavigationLink {
                            ThreadDetailView(eventID: note.id)
                        } label: {
                            CompactNoteRow(item: note)
                        }
                        .accessibilityIdentifier("profile-thread-link")
                    }
                } else {
                    Text("Waiting for author notes")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .navigationTitle(view?.profile.display ?? shortPubkeyDisplay(pubkey))
        .navigationBarTitleDisplayMode(.inline)
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(Color(uiColor: .systemGroupedBackground))
        .task(id: pubkey) {
            model.openAuthor(pubkey: pubkey)
        }
        .onDisappear {
            model.closeAuthor(pubkey: pubkey)
        }
        .accessibilityIdentifier("profile-detail-list")
    }
}

struct CompactNoteRow: View {
    let item: TimelineItem

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(item.contentPreview)
                .font(.footnote)
                .lineLimit(3)
            HStack(spacing: 8) {
                Text(item.createdAtDisplay)
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.secondary)
                Label("Thread", systemImage: "text.bubble")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("profile-thread-link")
            }
        }
        .padding(.vertical, 4)
    }
}
