import SwiftUI

/// Picker the user sees when sharing into Highlighter from another app.
/// Reads joined communities from the App Group cache (the main app mirrors
/// them on every refresh), lets the user pick one and optionally type a
/// comment, then hands the payload back to the containing extension to
/// enqueue + bounce to the main app.
struct ShareRootView: View {
    let incomingURL: URL?
    let onSubmit: (PendingShare) -> Void
    let onCancel: () -> Void

    @State private var communities: [SharedCommunitySummary] = SharedCommunitiesCache.load()
    @State private var selectedGroupId: String?
    @State private var note: String = ""

    private var canSubmit: Bool {
        incomingURL != nil && selectedGroupId != nil
    }

    var body: some View {
        NavigationStack {
            Group {
                if communities.isEmpty {
                    emptyState
                } else {
                    Form {
                        Section("Sharing") {
                            if let url = incomingURL {
                                Text(url.absoluteString)
                                    .font(.footnote)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(3)
                            } else {
                                Text("No URL was shared.")
                                    .foregroundStyle(.red)
                            }
                        }
                        Section("Add a note") {
                            TextField("Optional", text: $note, axis: .vertical)
                                .lineLimit(3, reservesSpace: true)
                        }
                        Section("Send to community") {
                            ForEach(communities, id: \.id) { c in
                                Button {
                                    selectedGroupId = c.id
                                } label: {
                                    HStack(spacing: 12) {
                                        avatar(for: c)
                                        VStack(alignment: .leading, spacing: 2) {
                                            Text(c.name).font(.body.weight(.medium))
                                            Text(c.id).font(.caption2).foregroundStyle(.secondary)
                                        }
                                        Spacer()
                                        if selectedGroupId == c.id {
                                            Image(systemName: "checkmark.circle.fill")
                                                .foregroundStyle(.tint)
                                        }
                                    }
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
            }
            .navigationTitle("Share")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { onCancel() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") { submit() }
                        .disabled(!canSubmit)
                }
            }
        }
    }

    private var emptyState: some View {
        ContentUnavailableView(
            "Open Highlighter first",
            systemImage: "person.crop.circle.badge.exclamationmark",
            description: Text("Sign in and join a community in the main app, then try sharing again.")
        )
    }

    @ViewBuilder
    private func avatar(for community: SharedCommunitySummary) -> some View {
        if let url = URL(string: community.picture), !community.picture.isEmpty {
            AsyncImage(url: url) { img in
                img.resizable().scaledToFill()
            } placeholder: {
                Color.secondary.opacity(0.1)
            }
            .frame(width: 36, height: 36)
            .clipShape(.rect(cornerRadius: 8))
        } else {
            RoundedRectangle(cornerRadius: 8)
                .fill(.tertiary)
                .frame(width: 36, height: 36)
        }
    }

    private func submit() {
        guard let url = incomingURL, let groupId = selectedGroupId else { return }
        let share = PendingShare(
            groupId: groupId,
            url: url.absoluteString,
            note: note.trimmingCharacters(in: .whitespaces)
        )
        onSubmit(share)
    }
}
