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
                    .padding(ChirpSpace.l)
                    .chirpGlass(cornerRadius: ChirpSpace.radius)
                    .padding(.horizontal, ChirpSpace.l)
                    .padding(.bottom, 8)

                notesSection
            }
            .padding(.top, ChirpSpace.m)
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
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .bottom, spacing: 0) {
                ChirpAvatar(
                    url: profile?.pictureUrl,
                    initials: profile?.avatarInitials ?? "?",
                    colorHex: profile?.avatarColor ?? "7B66FF",
                    size: 82
                )
                .padding(.leading, 16)

                Spacer()

                if let primaryAction {
                    Button {
                        performProfileAction(primaryAction)
                    } label: {
                        Label(primaryAction.label, systemImage: iconName(for: primaryAction))
                            .labelStyle(.titleAndIcon)
                    }
                    .buttonStyle(ChirpGlassButtonStyle(prominent: primaryAction.kind == "follow"))
                    .padding(.trailing, 16)
                }
            }
            .padding(.top, 16)

            // Meta block below avatar
            VStack(alignment: .leading, spacing: 4) {
                // Display name
                Text(profile?.display ?? "Loading…")
                    .font(.title)
                    .foregroundStyle(.primary)

                // NIP-05 verified badge
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

                // npub — monospaced, truncated, tappable to copy
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

                // About / bio
                if let about = profile?.about, !about.isEmpty {
                    Text(about)
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, 4)
                }
            }
            .padding(.horizontal, 16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
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
                HStack {
                    Text("Posts")
                        .font(.headline)
                        .foregroundStyle(.primary)
                    Spacer()
                    Text("\(items.count)")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("profile-notes-count-value")
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)

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
        withAnimation(.smooth(duration: 0.2)) { copiedNpub = true }
        Task {
            try? await Task.sleep(for: .seconds(2))
            withAnimation(.smooth(duration: 0.3)) { copiedNpub = false }
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
