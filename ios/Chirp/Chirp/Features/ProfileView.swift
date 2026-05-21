import SwiftUI

// OWNER: Phase-2 Agent B (Profile screen). Init fixed by nav: ProfileView(pubkey:).
//
// Thin-shell rule (aim.md §6.9): no business logic in Swift. Rust authors
// the primary-action label/icon/dispatch (`profile_action_for`), the post-
// count display string (`note_count_display`), the truncated npub
// (`ProfileCard.npub_short`), and the per-author mention map
// (`projections["mention_profiles"]`).

struct ProfileView: View {
    let pubkey: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var copiedNpub = false
    @State private var isEditingProfile = false

    private var authorView: AuthorProfileSnapshot? {
        model.authorView?.pubkey == pubkey ? model.authorView : nil
    }
    private var profile: ProfileCard? { authorView?.profile }
    private var items: [TimelineItem] { authorView?.items ?? [] }
    private var primaryAction: ProfileAction? { authorView?.primaryAction }

    /// Render context fed to each `ProfileNoteRow`. `mentionProfiles` is the
    /// Rust-derived projection (aim.md §4.2); the two remaining lookups are
    /// folded into one context built once per body pass.
    private var noteRenderContext: NoteRenderContext {
        NoteRenderContext(
            mentionProfiles: model.mentionProfiles,
            eventCards: Dictionary(
                uniqueKeysWithValues: model.modularTimeline.cards.map { ($0.id, $0) }),
            timelineItems: Dictionary(uniqueKeysWithValues: items.map { ($0.id, $0) }),
            embedDepth: 0
        )
    }

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
            // T152: release the author sub on nav-away (wire_subs baseline).
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
                .fill(
                    LinearGradient(
                        colors: [
                            (Color(hex: profile?.avatarColor ?? "7B66FF") ?? .accentColor).opacity(0.28),
                            Color(.secondarySystemBackground)
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
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
                    perform(primaryAction)
                } label: {
                    // label + iconName both authored by Rust — no Swift
                    // `switch action.kind` over SF Symbol names.
                    Label(primaryAction.label, systemImage: primaryAction.iconName)
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

            if profile?.hasProfile == true, let nip05 = profile?.nip05, !nip05.isEmpty {
                HStack(spacing: 4) {
                    Image(systemName: "checkmark.seal.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Color.accentColor)
                    Text(nip05)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }

            if let profile, !profile.npub.isEmpty {
                Button(action: copyNpub) {
                    HStack(spacing: 4) {
                        // Rust-truncated; no Swift formatter.
                        Text(profile.npubShort)
                            .font(.body.monospaced())
                            .foregroundStyle(.secondary)
                        Image(systemName: copiedNpub ? "checkmark" : "doc.on.doc")
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                    }
                }
                .buttonStyle(.plain)
            }

            if profile?.hasProfile == true, let about = profile?.about, !about.isEmpty {
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
            let context = noteRenderContext
            LazyVStack(spacing: 0) {
                VStack(spacing: 8) {
                    HStack(spacing: 6) {
                        Text("Posts")
                            .font(.headline)
                            .foregroundStyle(.primary)
                        // `noteCountDisplay` is Rust-formatted — no `\(items.count)`.
                        Text(authorView?.noteCountDisplay ?? "")
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
                        contentTree: context.eventCards[item.id]?.contentTree,
                        renderContext: context,
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

    /// Branches on presence-of-dispatch (write vs local intent) — NOT on
    /// `action.kind` (aim.md §4.4: writes flow through registered
    /// ActionModules, shell binds blindly).
    private func perform(_ action: ProfileAction) {
        if let dispatch = action.dispatch {
            model.dispatchProfileAction(dispatch)
            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        } else {
            isEditingProfile = true
        }
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
