import SwiftUI

// OWNER: Phase-2 Agent B (Profile screen).
// Init signature FIXED by nav contract: ProfileView(pubkey:).

struct ProfileView: View {
    let pubkey: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var copiedNpub = false
    @State private var replyToID: String? = nil
    @State private var isEditingProfile = false

    private var authorView: AuthorProfileSnapshot? {
        model.authorView?.pubkey == pubkey ? model.authorView : nil
    }
    private var profile: ProfileCard? { authorView?.profile }
    private var items: [TimelineItem] { authorView?.items ?? [] }
    private var primaryAction: ProfileAction? { authorView?.primaryAction }

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                profileHeader
                Divider()

                notesSection
            }
        }
        .accessibilityIdentifier("profile-detail-list")
        .chirpScreenBackground()
        .navigationTitle(profile?.display ?? "Profile")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            model.openAuthor(pubkey: pubkey)
        }
        .onDisappear {
            // T152: release the author subscription when this view is no
            // longer visible (NavigationStack pop, or another view pushed
            // on top).  Keeps wire_subs at baseline after navigation.
            model.closeAuthor(pubkey: pubkey)
        }
        .animation(.smooth(duration: 0.3), value: profile)
        .animation(.smooth(duration: 0.25), value: items.count)
        .sheet(isPresented: $isEditingProfile) {
            ProfileEditSheet(profile: profile) { name, about, picture in
                model.publishProfile(name: name, about: about, picture: picture)
            }
        }
    }

    // MARK: – Header

    @ViewBuilder
    private var profileHeader: some View {
        VStack(alignment: .leading, spacing: 0) {
            Rectangle()
                .fill(Color(.secondarySystemBackground))
                .frame(height: 118)
                .overlay(alignment: .bottom) {
                    Divider()
                }

            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .bottom) {
                    ChirpAvatar(
                        url: profile?.pictureUrl,
                        initials: profile?.avatarInitials ?? "?",
                        colorHex: profile?.avatarColor ?? "7B66FF",
                        size: 82
                    )
                    .padding(.top, -41)

                    Spacer()

                    profileActions
                        .padding(.top, 8)
                }

                profileMetadata
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private var profileActions: some View {
        if let primaryAction {
            HStack(spacing: 8) {
                Button {
                    performProfileAction(primaryAction)
                } label: {
                    Label(primaryAction.label, systemImage: iconName(for: primaryAction))
                        .font(.callout.weight(.semibold))
                        .labelStyle(.titleAndIcon)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .accessibilityLabel(primaryAction.label)
            }
        }
    }

    private var profileMetadata: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(profile?.display ?? "Loading…")
                .font(.title)
                .foregroundStyle(.primary)

            if let nip05 = profile?.nip05, !nip05.isEmpty {
                HStack(spacing: 4) {
                    Image(systemName: "checkmark.seal.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Color.accentColor)
                    Text(nip05)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }

            if let npub = profile?.npub, !npub.isEmpty {
                Button(action: copyNpub) {
                    HStack(spacing: 4) {
                        Text(truncatedNpub(npub))
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                        Image(systemName: copiedNpub ? "checkmark" : "doc.on.doc")
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                    }
                }
                .buttonStyle(.plain)
            }

            if let about = profile?.about, !about.isEmpty {
                Text(about)
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: – Notes list

    @ViewBuilder
    private var notesSection: some View {
        if items.isEmpty {
            ChirpPlaceholder(
                systemImage: "note.text",
                title: "No posts yet",
                subtitle: "Posts by this person will appear here."
            )
            .frame(minHeight: 260)
        } else {
            LazyVStack(spacing: 0) {
                VStack(spacing: 8) {
                    HStack(spacing: 6) {
                        Text("Posts")
                            .font(.headline)
                            .foregroundStyle(.primary)
                        Text("\(items.count)")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .accessibilityIdentifier("profile-notes-count-value")
                    }

                    Capsule()
                        .fill(.tint)
                        .frame(width: 36, height: 3)
                }
                .frame(maxWidth: .infinity)
                .padding(.top, 12)

                Divider()

                ForEach(items) { item in
                    ProfileNoteRow(
                        item: item,
                        onAvatarTap: {
                            router.push(.profile(pubkey: item.authorPubkey))
                        },
                        onRowTap: {
                            router.push(.thread(eventID: item.id))
                        },
                        onLike: {
                            model.react(targetEventID: item.id, reaction: "❤")
                        }
                    )

                    if item.id != items.last?.id {
                        Divider()
                            .padding(.leading, 68)
                            .opacity(0.35)
                    }
                }
            }
        }
    }

    // MARK: – Helpers

    private func performProfileAction(_ action: ProfileAction) {
        switch action.kind {
        case "edit_profile":
            isEditingProfile = true
        case "follow":
            model.follow(action.targetPubkey)
        case "unfollow":
            model.unfollow(action.targetPubkey)
        default:
            break
        }
    }

    private func iconName(for action: ProfileAction) -> String {
        switch action.kind {
        case "edit_profile":
            return "square.and.pencil"
        case "unfollow":
            return "person.badge.minus"
        default:
            return "person.badge.plus"
        }
    }

    private func truncatedNpub(_ npub: String) -> String {
        guard npub.count > 20 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(8))"
    }

    private func copyNpub() {
        guard let npub = profile?.npub else { return }
        UIPasteboard.general.string = npub
        copiedNpub = true
        Task {
            try? await Task.sleep(for: .seconds(2))
            copiedNpub = false
        }
    }
}

private struct ProfileEditSheet: View {
    let profile: ProfileCard?
    let onSave: (String, String, String) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @State private var about: String
    @State private var picture: String

    init(profile: ProfileCard?, onSave: @escaping (String, String, String) -> Void) {
        self.profile = profile
        self.onSave = onSave
        _name = State(initialValue: profile?.display ?? "")
        _about = State(initialValue: profile?.about ?? "")
        let pictureUrl = profile?.pictureUrl ?? ""
        _picture = State(initialValue: pictureUrl.hasPrefix("http") ? pictureUrl : "")
    }

    var body: some View {
        NavigationStack {
            Form {
                TextField("Name", text: $name)
                TextField("About", text: $about, axis: .vertical)
                    .lineLimit(3...6)
                TextField("Picture URL", text: $picture)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
            }
            .navigationTitle("Edit Profile")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        onSave(
                            name.trimmingCharacters(in: .whitespacesAndNewlines),
                            about.trimmingCharacters(in: .whitespacesAndNewlines),
                            picture.trimmingCharacters(in: .whitespacesAndNewlines)
                        )
                        dismiss()
                    }
                    .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
    }
}
