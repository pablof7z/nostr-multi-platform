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

    /// Rust-authored wire for the shared `NostrUserCard` primitive. Built from
    /// the same `ProfileCard` the header already reads (claimed → resolved);
    /// Swift never resolves display name / NIP-05 / avatar URL itself.
    private var profileWire: ProfileWire {
        let card = profile
        return ProfileWire(
            pubkey: pubkey,
            displayName: (card?.displayName?.isEmpty == false) ? card?.displayName : nil,
            about: (card?.about.isEmpty == false) ? card?.about : nil,
            pictureUrl: card?.pictureUrl,
            nip05: (card?.nip05.isEmpty == false) ? card?.nip05 : nil,
            npub: nil,
            npubShort: pubkey.shortHex
        )
    }

    /// True when the screen shows the signed-in account's own profile.
    private var isOwnProfile: Bool { model.activeAccount == pubkey }

    /// True when the active account's NIP-02 follow set already contains
    /// `pubkey`. Reads the same `FollowListStore` the kernel snapshot feeds
    /// (`model.followList.follows`); the shell never recomputes graph state.
    private var isFollowing: Bool {
        model.followList.follows.contains { $0.pubkey == pubkey }
    }

    /// The single primary button rendered over the profile header. Own profile
    /// ⇒ the local `edit_profile` intent (opens the edit sheet, `dispatch ==
    /// nil`); another account ⇒ Follow / Unfollow, whose `perform()` routes
    /// through the existing typed `KernelModel.follow(_:)` / `unfollow(_:)`
    /// helpers (the `nmp-app-chirp` ActionModule seam). The shell authors only
    /// the label/icon for presentation; it never builds namespaces or bodies.
    private var primaryAction: ProfileAction? {
        if isOwnProfile {
            return ProfileAction(
                kind: "edit_profile",
                label: "Edit Profile",
                targetPubkey: pubkey,
                iconName: "pencil",
                dispatch: nil
            )
        }
        if isFollowing {
            return ProfileAction(
                kind: "unfollow",
                label: "Following",
                targetPubkey: pubkey,
                iconName: "checkmark",
                dispatch: nil
            )
        }
        return ProfileAction(
            kind: "follow",
            label: "Follow",
            targetPubkey: pubkey,
            iconName: "person.badge.plus",
            dispatch: nil
        )
    }

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
            // Screen-specific chrome: gradient banner the avatar overlaps.
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
                // Shared NMP primitive: avatar + display name + NIP-05 badge.
                // The actions overlay floats on top so it tracks the banner edge.
                NostrUserCard(profile: profileWire, avatarSize: 82)
                    .padding(.top, -41)
                    .overlay(alignment: .topTrailing) {
                        profileActions
                            .padding(.top, 8)
                    }

                // Screen-specific chrome: npub copy + about.
                profileChrome
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

    /// Genuinely screen-specific chrome kept outside `NostrUserCard`: the
    /// npub-copy chip and the about blurb. Avatar, display name, and the NIP-05
    /// badge now come from the shared primitive (issue #995).
    private var profileChrome: some View {
        VStack(alignment: .leading, spacing: 4) {
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

    /// Routes the primary action to the existing typed write helpers.
    ///
    /// V-112 (ADR-0042) deleted the Rust `profile_action_for` authoring, so
    /// follow/unfollow now flow through the `nmp-app-chirp` ActionModule seam
    /// directly via `KernelModel.follow(_:)` / `unfollow(_:)` — Rust still
    /// authors the namespace + body inside `nmp_app_chirp_action_spec`; the
    /// shell only forwards the raw pubkey, exactly like the React / Repost row
    /// buttons. `edit_profile` is a local UI intent (no write).
    private func perform(_ action: ProfileAction) {
        switch action.kind {
        case "follow":
            model.follow(action.targetPubkey)
            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        case "unfollow":
            model.unfollow(action.targetPubkey)
            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        default:
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
