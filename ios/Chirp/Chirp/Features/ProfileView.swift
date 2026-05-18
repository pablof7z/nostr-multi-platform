import SwiftUI

// OWNER: Phase-2 Agent B (Profile screen).
// Init signature FIXED by nav contract: ProfileView(pubkey:).
//
// NOTE: claimProfile/releaseProfile are available on KernelHandle (Bridge)
// but not on KernelModel's public surface. We call openAuthor only.
// Follow-state is not in the model snapshot; the Follow pill is always shown
// (no synthesised local state per D8).

struct ProfileView: View {
    let pubkey: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var copiedNpub = false
    @State private var replyToID: String? = nil

    private var profile: ProfileCard? { model.profile }
    private var isPlaceholder: Bool { model.profile?.source == "placeholder" }

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                profileHeader
                    .padding(.bottom, ChirpSpace.s)

                Divider()
                    .background(ChirpColor.hairline)

                notesSection
            }
        }
        .accessibilityIdentifier("profile-detail-list")
        .background(ChirpColor.bg.ignoresSafeArea())
        .navigationTitle(profile?.display ?? "Profile")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            model.openAuthor(pubkey: pubkey)
        }
        .animation(.smooth(duration: 0.3), value: model.profile)
        .animation(.smooth(duration: 0.25), value: model.items.count)
    }

    // MARK: – Header

    @ViewBuilder
    private var profileHeader: some View {
        ZStack(alignment: .bottomLeading) {
            // Banner gradient
            bannerGradient
                .frame(height: 140)
                .clipped()

            // Avatar overlapping the banner bottom edge
            HStack(alignment: .bottom, spacing: 0) {
                ChirpAvatar(
                    url: profile?.pictureUrl,
                    initials: profile?.avatarInitials ?? "?",
                    colorHex: profile?.avatarColor ?? "7B66FF",
                    size: 82
                )
                .overlay(
                    Circle()
                        .strokeBorder(ChirpColor.bg, lineWidth: 3)
                )
                .offset(y: 28)
                .padding(.leading, ChirpSpace.l)

                Spacer()

                // Follow action pill aligned to bottom of banner
                followPill
                    .padding(.trailing, ChirpSpace.l)
                    .padding(.bottom, ChirpSpace.xs)
            }
        }
        .padding(.bottom, 36) // room for avatar overflow

        // Meta block below avatar
        VStack(alignment: .leading, spacing: ChirpSpace.xs) {
            // Display name
            Text(profile?.display ?? "Loading…")
                .font(ChirpFont.title)
                .foregroundStyle(ChirpColor.textPrimary)
                .redacted(reason: isPlaceholder ? .placeholder : [])

            // NIP-05 verified badge
            if let nip05 = profile?.nip05, !nip05.isEmpty {
                HStack(spacing: ChirpSpace.xs) {
                    Image(systemName: "checkmark.seal.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(ChirpColor.accent)
                    Text(nip05)
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textSecondary)
                }
            }

            // npub — monospaced, truncated, tappable to copy
            if let npub = profile?.npub, !npub.isEmpty {
                Button(action: copyNpub) {
                    HStack(spacing: ChirpSpace.xs) {
                        Text(truncatedNpub(npub))
                            .font(ChirpFont.mono)
                            .foregroundStyle(ChirpColor.textTertiary)
                        Image(systemName: copiedNpub ? "checkmark" : "doc.on.doc")
                            .font(.system(size: 11))
                            .foregroundStyle(copiedNpub ? ChirpColor.positive : ChirpColor.textTertiary)
                            .animation(.bouncy, value: copiedNpub)
                    }
                }
                .buttonStyle(.plain)
            }

            // About / bio
            if let about = profile?.about, !about.isEmpty {
                Text(about)
                    .font(ChirpFont.body)
                    .foregroundStyle(ChirpColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, ChirpSpace.xs)
                    .redacted(reason: isPlaceholder ? .placeholder : [])
            }
        }
        .padding(.horizontal, ChirpSpace.l)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private var bannerGradient: some View {
        Group {
            if let profile {
                ChirpColor.avatar(from: profile.avatarColor)
            } else {
                ChirpColor.avatar(from: "7B66FF")
            }
        }
        .overlay(
            LinearGradient(
                colors: [Color.clear, ChirpColor.bg.opacity(0.55)],
                startPoint: .top,
                endPoint: .bottom
            )
        )
    }

    @ViewBuilder
    private var followPill: some View {
        HStack(spacing: ChirpSpace.s) {
            Button {
                model.follow(pubkey)
            } label: {
                Text("Follow")
                    .font(ChirpFont.headline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, ChirpSpace.l)
                    .padding(.vertical, 8)
                    .background(ChirpColor.accent, in: Capsule())
            }
            .buttonStyle(.plain)

            Button {
                model.unfollow(pubkey)
            } label: {
                Image(systemName: "person.badge.minus")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(ChirpColor.textSecondary)
                    .padding(8)
                    .background(.ultraThinMaterial, in: Circle())
                    .overlay(Circle().strokeBorder(ChirpColor.hairline))
            }
            .buttonStyle(.plain)
        }
    }

    // MARK: – Notes list

    @ViewBuilder
    private var notesSection: some View {
        if model.items.isEmpty {
            ChirpPlaceholder(
                systemImage: "note.text",
                title: "No posts yet",
                subtitle: "Posts by this person will appear here."
            )
            .frame(minHeight: 260)
        } else {
            LazyVStack(spacing: 0) {
                HStack {
                    ChirpSectionHeader(title: "Posts")
                    Spacer()
                    Text("\(model.items.count)")
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textTertiary)
                        .accessibilityIdentifier("profile-notes-count-value")
                }
                .padding(.horizontal, ChirpSpace.l)
                .padding(.vertical, ChirpSpace.m)

                ForEach(model.items) { item in
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

                    if item.id != model.items.last?.id {
                        Divider()
                            .background(ChirpColor.hairline)
                            .padding(.leading, 68)
                    }
                }
            }
        }
    }

    // MARK: – Helpers

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
