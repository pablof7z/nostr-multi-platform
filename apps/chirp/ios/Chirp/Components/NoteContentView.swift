import SwiftUI

private struct TappedImage: Identifiable {
    let url: URL
    var id: String { url.absoluteString }
}

struct NoteContentView: View {
    let content: String
    let contentTree: ContentTreeWire?
    let renderContext: NoteRenderContext
    var font: Font = .body

    @EnvironmentObject private var router: ChirpRouter
    @Environment(\.nostrProfileHost) private var profileHost
    @State private var tappedImage: TappedImage?

    /// Stable consumer-id for this content view's mention claims. Lives in
    /// `@State` so it survives re-renders; every claim made under it is matched
    /// by a release in `releaseAllMentions` (refcount discipline — mirror of
    /// `NostrAvatar`).
    @State private var mentionConsumerID = "note-content.mentions.\(UUID().uuidString)"
    /// Pubkeys currently claimed under `mentionConsumerID`, so a tree change or
    /// disappear releases exactly what was claimed.
    @State private var claimedMentions: [String] = []
    /// ADR-0063 Lane E (#1671): per-key observer for THIS note's mention
    /// pubkeys. Inline mentions render as a single concatenated `Text`, so no
    /// per-mention SwiftUI view can host its own observer (like
    /// `NostrProfileName` does). Instead this one note's content view observes
    /// exactly its mention rows in `refs.profile`; when any of them commits, it
    /// re-evaluates its body and re-reads the label from the keyed-ref cache.
    /// A profile update therefore re-renders ONLY the notes that mention that
    /// pubkey — never the whole view tree (no whole-map `@Published` broadcast).
    @StateObject private var mentionObserver = KeyedRefMultiRowObserver()

    init(
        content: String,
        contentTree: ContentTreeWire? = nil,
        eventCards: [String: ChirpEventCard] = [:],
        renderContext: NoteRenderContext? = nil,
        font: Font = .body
    ) {
        self.content = content
        self.contentTree = contentTree
        self.renderContext = renderContext ?? NoteRenderContext(eventCards: eventCards)
        self.font = font
    }

    var body: some View {
        Group {
            if let contentTree {
                richBody(contentTree)
            } else {
                plainBody
            }
        }
        .fullScreenCover(item: $tappedImage) { item in
            FullScreenImageViewer(url: item.url)
        }
        // F-CR-00 claim-only invariant: claim the kind:0 of every profile
        // mention this note renders, mirroring `NostrAvatar`'s lifecycle. The
        // mention labels render as inline `Text` runs (no per-mention view), so
        // the claim is hoisted here, keyed off the mention pubkey set so a tree
        // change re-claims and `onDisappear` releases. The kernel owns all
        // resolution (10002 → author relays → kind:0 REQ) off the claim alone.
        .task(id: mentionPubkeys) {
            await MainActor.run { syncMentionClaims(to: mentionPubkeys) }
        }
        .onDisappear {
            releaseAllMentions()
        }
    }

    /// Render the inline label for one profile mention from the keyed-ref cache
    /// (`profileHost.profile(forPubkey:)`), NOT a whole-map projection. Per-key
    /// reactivity comes from `mentionObserver`, which re-renders this note when
    /// the mentioned pubkey's `refs.profile` row commits. Falls back to the
    /// shortened pubkey while the kind:0 is unresolved — never blank.
    private func mentionLabel(for pubkey: String) -> String {
        profileHost?.profile(forPubkey: pubkey)?.display ?? shortEntity(pubkey)
    }

    /// Profile mentions in the currently-rendered tree (empty for plain text).
    private var mentionPubkeys: [String] {
        contentTree?.mentionPubkeys ?? []
    }

    /// Reconcile the live claim set to `target`: release pubkeys no longer
    /// present, claim newly-present ones. Idempotent re-claims are cheap
    /// (kernel dedups by consumer-id).
    private func syncMentionClaims(to target: [String]) {
        guard let profileHost else { return }
        let targetSet = Set(target)
        let currentSet = Set(claimedMentions)
        for pk in currentSet.subtracting(targetSet) {
            profileHost.releaseProfile(pubkey: pk, consumerID: mentionConsumerID)
        }
        for pk in targetSet.subtracting(currentSet) {
            // ADR-0063 Lane E (#1671): inline mentions are a list/reading
            // context → `resolve_ref(Profile, …, .profileRef, .cacheOk)` (no
            // live sub; cache + OneShot fill is sufficient).
            profileHost.resolveProfile(
                pubkey: pk, consumerID: mentionConsumerID,
                shape: .profileRef, liveness: .cacheOk)
        }
        claimedMentions = target
        // ADR-0063 Lane E (#1671): bind the per-key observer to exactly this
        // note's mention pubkeys so a kind:0 arrival for one of them re-renders
        // only this note (re-reading the keyed cache), not the whole tree.
        mentionObserver.observe(profileHost.profileRowChanged, pubkeys: targetSet)
    }

    private func releaseAllMentions() {
        guard let profileHost else { return }
        for pk in claimedMentions {
            profileHost.releaseProfile(pubkey: pk, consumerID: mentionConsumerID)
        }
        claimedMentions = []
    }

    @ViewBuilder
    private func richBody(_ tree: ContentTreeWire) -> some View {
        // Embed refs flow through the NostrKindRegistry environment path
        // (EmbeddedEvent) injected by ChirpApp via `.embedEnvelopeSource(...)`,
        // so quote cards use the same kind-registry seam as article/highlight
        // embeds (ADR-0034 / F-CR-04 — the legacy quote-card path is deleted).
        NostrContentView(
            tree: tree,
            font: font,
            mentionLabel: { uri in mentionLabel(for: uri.primaryId) }
        )
        .nostrContentRenderer(chirpContentRenderer)
    }

    private var plainBody: some View {
        Text(content)
            .font(font)
    }

    private var chirpContentRenderer: NostrContentRenderer {
        NostrContentRenderer(
            textColor: .primary,
            secondaryTextColor: .secondary,
            mentionColor: ChirpColor.link,
            hashtagColor: ChirpColor.link,
            linkColor: ChirpColor.link,
            quoteBorderColor: ChirpColor.hairline.opacity(0.55),
            quoteBackgroundColor: ChirpColor.surface.opacity(0.75),
            codeBackgroundColor: ChirpColor.secondaryFill,
            placeholderColor: .secondary,
            callbacks: NostrContentCallbacks(
                onImageTap: { url in tappedImage = TappedImage(url: url) },
                onEventRefTap: { eventID in router.push(.thread(eventID: eventID)) }
            )
        )
    }
}

private struct FullScreenImageViewer: View {
    let url: URL
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack(alignment: .topTrailing) {
            ChirpColor.mediaBackdrop.ignoresSafeArea()
            AsyncImage(url: url) { phase in
                if let img = phase.image {
                    img.resizable()
                        .scaledToFit()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if phase.error != nil {
                    VStack(spacing: 12) {
                        Image(systemName: "photo.badge.exclamationmark")
                            .font(.system(size: 48, weight: .light))
                        Text("Image unavailable")
                            .font(.callout)
                    }
                    .foregroundStyle(.secondary)
                } else {
                    ProgressView().tint(ChirpColor.mediaForeground)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .font(.title)
                    .symbolRenderingMode(.palette)
                    .foregroundStyle(ChirpColor.mediaForeground, ChirpColor.mediaSecondaryForeground)
                    .padding(20)
            }
        }
        .onTapGesture { dismiss() }
    }
}
