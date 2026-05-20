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
        .animation(.smooth(duration: 0.3), value: model.profile)
        .animation(.smooth(duration: 0.25), value: model.items.count)
        .toolbar {
            ToolbarItemGroup(placement: .navigationBarTrailing) {
                Button {
                    model.follow(pubkey)
                } label: {
                    Text("Follow")
                }

                Button {
                    model.unfollow(pubkey)
                } label: {
                    Image(systemName: "person.badge.minus")
                }
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
            }
            .padding(.top, 16)

            // Meta block below avatar
            VStack(alignment: .leading, spacing: 4) {
                // Display name
                Text(profile?.display ?? "Loading…")
                    .font(.title)
                    .foregroundStyle(.primary)
                    .redacted(reason: isPlaceholder ? .placeholder : [])

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
                        .redacted(reason: isPlaceholder ? .placeholder : [])
                }
            }
            .padding(.horizontal, 16)
            .frame(maxWidth: .infinity, alignment: .leading)
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
                    Text("Posts")
                        .font(.headline)
                        .foregroundStyle(.primary)
                    Spacer()
                    Text("\(model.items.count)")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .accessibilityIdentifier("profile-notes-count-value")
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)

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
                            .padding(.leading, 68)
                            .opacity(0.35)
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
