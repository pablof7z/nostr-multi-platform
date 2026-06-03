import SwiftUI

// OWNER: Phase-2 Agent B (Profile screen). Init fixed by nav: ProfileView(pubkey:).
//
// M2 Step-C (V-112, ADR-0042 §5): the notes list now reads from the dynamic
// per-author feed projection `nmp.feed.author.<pubkey>` (registered by
// `openAuthorFeed`) instead of the static `author_view` snapshot. The profile
// CARD (name / bio / nip05 / npub) still comes from `claimed_profiles` — opening
// the author feed admits kind:1/6 only, so this view force-claims the author's
// kind:0 on appear so the header populates for non-followed visited profiles.
//
// Thin-shell rule (aim.md §6.9): the FOLLOW WRITE still flows through Rust
// (`model.follow`/`model.unfollow` → kernel composes the kind:3 mutation). Only
// the follow-STATE read and the Follow/Following label+icon choice are local —
// ADR-0032 presentation-layer formatting of a boolean (precedent: the local
// like-state toggle in `ProfileNoteRow`). With `author_view` abandoned, the
// note-count string is likewise derived from the card count here.

struct ProfileView: View {
    let pubkey: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var copiedNpub = false
    @State private var isEditingProfile = false

    /// Consumer id for the kind:0 claim taken while this profile is on-screen.
    private var profileClaimConsumerID: String { "ios.profile-view:\(pubkey)" }

    /// `nmp.feed.author.<pubkey>` — the dynamic per-author feed key opened by
    /// `openAuthorFeed`. Read through `model.feedProjection(key:)`.
    private var authorFeedKey: String { "nmp.feed.author.\(pubkey)" }

    /// The author's profile card from `claimed_profiles` (the kind:0 self-claim
    /// taken on appear). Unchanged source for name / bio / nip05 / npub.
    private var profile: ProfileCard? { model.claimedProfiles[pubkey] }

    /// Notes for this author, derived from the dynamic feed projection. The flat
    /// feed is `RootFeedSnapshot` (`cards: [ChirpRootCard]`, attribution always
    /// empty for the author feed); each inner card becomes a synthetic
    /// `TimelineItem` the existing `ProfileNoteRow` renders.
    private var items: [TimelineItem] {
        (model.feedProjection(key: authorFeedKey)?.cards ?? [])
            .map { TimelineItem(card: $0.card) }
    }

    /// True when `pubkey` is the active account's own profile — drives the
    /// "Edit Profile" affordance instead of Follow/Unfollow.
    private var isOwnProfile: Bool { model.activeAccount == pubkey }

    /// Local follow-state read of the active account's kind:3 contact list. The
    /// WRITE still routes through Rust (`model.follow`/`unfollow`).
    private var isFollowing: Bool {
        model.followList.follows.contains { $0.pubkey == pubkey }
    }

    /// Render context fed to each `ProfileNoteRow`. `mentionProfiles` is the
    /// Rust-derived projection (aim.md §4.2). `eventCards` is the per-author
    /// feed's own cards (its primary source post-migration); `timelineItems`
    /// is the synthetic items built from those cards.
    private var noteRenderContext: NoteRenderContext {
        let feedCards = model.feedProjection(key: authorFeedKey)?.cards ?? []
        return NoteRenderContext(
            mentionProfiles: model.mentionProfiles,
            eventCards: Dictionary(
                uniqueKeysWithValues: feedCards.map { ($0.card.id, $0.card) }),
            timelineItems: Dictionary(uniqueKeysWithValues: items.map { ($0.id, $0) })
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
            // M2 Step-C: open the dynamic per-author feed (kind:1/6 of this
            // author → `nmp.feed.author.<pubkey>`). Force-claim the author's
            // kind:0 too — the feed admits notes only, so the profile card
            // (bio / nip05 / npub) needs an explicit claim to populate for a
            // non-followed visited profile.
            model.openAuthorFeed(pubkey: pubkey)
            model.claimProfile(pubkey: pubkey, consumerID: profileClaimConsumerID)
        }
        .onDisappear {
            // T152: release the author feed + kind:0 claim on nav-away so the
            // kernel's wire_subs / claim counts return to baseline.
            model.closeAuthorFeed(pubkey: pubkey)
            model.releaseProfile(pubkey: pubkey, consumerID: profileClaimConsumerID)
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
        // M2 Step-C: the Rust-authored `primaryAction` died with the
        // `author_view` read. The self-vs-other branch + Follow/Following label
        // are now derived locally (ADR-0032 presentation formatting of booleans);
        // the WRITE still routes through Rust (`model.follow`/`unfollow`).
        HStack(spacing: 8) {
            if isOwnProfile {
                Button {
                    isEditingProfile = true
                } label: {
                    Label("Edit Profile", systemImage: "pencil")
                        .font(.callout.weight(.semibold))
                        .labelStyle(.titleAndIcon)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .accessibilityLabel("Edit Profile")
            } else {
                let following = isFollowing
                Button {
                    if following {
                        model.unfollow(pubkey)
                    } else {
                        model.follow(pubkey)
                    }
                    UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                } label: {
                    Label(
                        following ? "Following" : "Follow",
                        systemImage: following ? "checkmark" : "plus"
                    )
                    .font(.callout.weight(.semibold))
                    .labelStyle(.titleAndIcon)
                }
                // `.borderedProminent` keeps the type concrete (no
                // `AnyButtonStyle` erasure); the tint swap de-emphasises the
                // button once following (accent CTA → neutral surface) and
                // `.borderedProminent` derives the readable label colour from
                // the tint automatically.
                .buttonStyle(.borderedProminent)
                .tint(following ? ChirpColor.surface : ChirpColor.accent)
                .controlSize(.small)
                .accessibilityLabel(following ? "Following" : "Follow")
            }
        }
    }

    private var profileMetadata: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(profile?.displayLabel ?? "Loading…")
                .font(.title)
                .foregroundStyle(.primary)

            if profile?.hasProfile == true, let nip05 = profile?.nip05, !nip05.isEmpty {
                HStack(spacing: 4) {
                    Image(systemName: "checkmark.seal.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(ChirpColor.success)
                    Text(nip05)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }

            if let profile, !profile.npub.isEmpty {
                Button(action: copyNpub) {
                    HStack(spacing: 4) {
                        // ADR-0032: shell-side abbreviation of the bech32 npub.
                        Text(profile.npub.shortHex)
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
                        // M2 Step-C: with `author_view.noteCountDisplay`
                        // abandoned, the count is derived from the feed card
                        // count. ADR-0032 presentation formatting (a plain
                        // integer needs no locale-sensitive Rust formatter).
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
                        },
                        onRepost: {
                            model.repost(eventID: item.id, authorPubkey: item.authorPubkey)
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
