import SwiftUI

// OWNER: Phase-2 Agent B (Profile screen). Init fixed by nav: ProfileView(pubkey:).
//
// Thin-shell rule: no business logic in Swift. Rust authors the flat author
// feed membership under `nmp.feed.author.<pubkey>`; Swift formats raw
// profile/count fields for presentation (ADR-0032).

struct ProfileView: View {
    let pubkey: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var copiedNpub = false
    @State private var isEditingProfile = false

    private var profileConsumerID: String { "profile-screen-\(pubkey)" }
    private var profile: ProfileCard? { model.claimedProfiles[pubkey] ?? model.resolvedProfileCards[pubkey] }
    private var items: [ChirpRootCard] { model.authorFeed(pubkey: pubkey)?.cards ?? [] }
    private var primaryAction: ProfileAction? { nil }

    /// Render context fed to each `ProfileNoteRow`. `mentionProfiles` is the
    /// Rust-derived projection (aim.md §4.2); the two remaining lookups are
    /// folded into one context built once per body pass.
    private var noteRenderContext: NoteRenderContext {
        NoteRenderContext(
            mentionProfiles: model.mentionProfiles,
            eventCards: Dictionary(uniqueKeysWithValues: items.map { ($0.card.id, $0.card) }),
            timelineItems: [:]
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
        .navigationTitle(profile?.displayLabel ?? "Profile")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            model.openAuthor(pubkey: pubkey)
            model.claimProfile(pubkey: pubkey, consumerID: profileConsumerID)
        }
        .onDisappear {
            // T152: release the author sub on nav-away (wire_subs baseline).
            model.closeAuthor(pubkey: pubkey)
            model.releaseProfile(pubkey: pubkey, consumerID: profileConsumerID)
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
                            ChirpColor.avatarBase(from: profile?.pubkey.pubkeyColorHex).opacity(0.28),
                            ChirpColor.surface
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
                        pubkey: profile?.pubkey ?? "",
                        url: profile?.pictureUrl,
                        initials: (profile?.displayLabel ?? "?").displayInitials,
                        colorHex: profile?.pubkey.pubkeyColorHex ?? "",
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
            Text(profile?.displayLabel ?? "Loading…")
                .font(.title)
                .foregroundStyle(.primary)

            if let nip05 = profile?.nip05, !nip05.isEmpty {
                HStack(spacing: 4) {
                    Image(systemName: "checkmark.seal.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(ChirpColor.success)
                    Text(nip05)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }

            // ADR-0032 / V-115: bech32 no longer in projection; encode host-side
            // on demand. Always show the copy button — pubkey is always available.
            Button(action: copyNpub) {
                HStack(spacing: 4) {
                    Text(pubkey.shortHex)
                        .font(.body.monospaced())
                        .foregroundStyle(.secondary)
                    Image(systemName: copiedNpub ? "checkmark" : "doc.on.doc")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
            }
            .buttonStyle(.plain)

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
            let context = noteRenderContext
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
                        .fill(ChirpColor.accent)
                        .frame(width: 36, height: 3)
                }
                .frame(maxWidth: .infinity)
                .padding(.top, 12)

                Divider()

                ForEach(items) { root in
                    let card = root.card
                    ProfileNoteRow(
                        card: card,
                        renderContext: context,
                        onAvatarTap: {
                            router.push(.profile(pubkey: card.authorPubkey))
                        },
                        onRowTap: {
                            router.push(.thread(eventID: card.id))
                        },
                        onLike: {
                            model.react(targetEventID: card.id, reaction: "❤")
                        },
                        onRepost: {
                            model.repost(eventID: card.id, authorPubkey: card.authorPubkey)
                        }
                    )

                    if root.id != items.last?.id {
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
        // ADR-0032 / V-115: encode bech32 host-side; fall back to hex if the
        // C function fails (e.g. invalid key or no app handle).
        let npub = model.encodeProfile(pubkey: pubkey) ?? pubkey
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
        // ADR-0032: edit sheet seeds the name with the kind:0 display name
        // (raw, may be `nil`) — NOT the abbreviated-hex fallback, which
        // would corrupt the published profile on save.
        _name = State(initialValue: profile?.displayName ?? "")
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
