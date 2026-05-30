import SwiftUI

/// Shared chrome for content-component pages.
private struct ContentPageFrame<Content: View>: View {
    let caption: String
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(caption)
                .font(.caption)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading) {
                content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(20)
            .background(Color(.secondarySystemGroupedBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }
}

/// Reusable toggle shown at the top of pages that render inline mentions.
/// When on, `NostrContentView` receives the raw wire URI; when off, the
/// kernel-backed display name (or truncated pubkey while loading).
private struct RawToggle: View {
    @Binding var rawMode: Bool

    var body: some View {
        HStack {
            Text(rawMode ? "Raw wire" : "Profile")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Spacer()
            Toggle("", isOn: $rawMode)
                .labelsHidden()
        }
    }
}

// MARK: - Showcase references

/// A reusable rich `ContentTreeWire` exercise. Mirrors the registry's
/// `NostrContentViewPreview` arena and uses the same relay-backed references
/// as the TUI and Android galleries.
private enum ShowcaseContent {
    static var richTree: ContentTreeWire {
        // Arena layout:
        //   0  text "relay note "
        //   1  mention(SHOWCASE_PUBKEY_HEX)
        //   2  text " "
        //   3  eventRef(SHOWCASE_NOTE_NEVENT)
        //   4  paragraph(children: [0,1,2,3])
        return ContentTreeWire(
            nodes: [
                .text("relay note "),
                .mention(
                    NostrWireUri(
                        uri: "nostr:\(SHOWCASE_NPUB)",
                        kind: .profile,
                        primaryId: SHOWCASE_PUBKEY_HEX
                    )
                ),
                .text(" "),
                .eventRef(showcaseNoteURI),
                .paragraph(children: [0, 1, 2, 3]),
            ],
            roots: [4],
            mode: nil
        )
    }
}

// MARK: - content-core

struct ContentCorePage: View {
    var body: some View {
        VStack(spacing: 16) {
            ContentPageFrame(caption: "ContentTreeWire — arena dump") {
                let tree = ShowcaseContent.richTree
                Text("nodes: \(tree.nodes.count)   roots: \(tree.roots.count)")
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                Text("Render it with NostrContentView; the wire is just data.")
                    .font(.callout)
            }
            ContentPageFrame(caption: "NostrIdenticon.identiconView(forPubkey:size:)") {
                HStack(spacing: 16) {
                    NostrIdenticon.identiconView(forPubkey: SHOWCASE_PUBKEY_HEX, size: 32)
                    NostrIdenticon.identiconView(forPubkey: SHOWCASE_PUBKEY_HEX, size: 40)
                    NostrIdenticon.identiconView(forPubkey: SHOWCASE_PUBKEY_HEX, size: 48)
                    NostrIdenticon.identiconView(forPubkey: SHOWCASE_PUBKEY_HEX, size: 56)
                }
            }
        }
    }
}

// MARK: - content-view

struct ContentViewPage: View {
    @Environment(GalleryModel.self) private var model
    @State private var rawMode = false

    var body: some View {
        VStack(spacing: 16) {
            // A hidden avatar owns the claim for the showcase pubkey while this
            // page is visible — same mechanism as `ProfileEmbedPage`, so
            // `model.profile(forPubkey:)` resolves to the display name instead
            // of the truncated hex fallback.
            NostrAvatar(pubkey: SHOWCASE_PUBKEY_HEX, size: 0)
                .frame(width: 0, height: 0)
                .clipped()
            RawToggle(rawMode: $rawMode)
            ContentPageFrame(caption: "NostrContentView(tree:)") {
                NostrContentView(
                    tree: ShowcaseContent.richTree,
                    mentionLabel: { uri in
                        rawMode
                            ? uri.uri
                            : model.profile(forPubkey: uri.primaryId)?.displayName
                                ?? NostrContentView.defaultMentionLabel(uri)
                    }
                )
            }
        }
    }
}

// MARK: - content-mention-chip

/// Inline-mention showcase: a `ContentTreeWire` that renders "Hey @pablof7z,
/// how are you?". The mention node's `primaryId` is `SHOWCASE_PUBKEY_HEX`
/// and the `mentionLabel` closure looks up the live kind:0 profile the
/// kernel claimed for the mounted component. Raw toggle shows the wire URI vs the
/// resolved display name.
private enum MentionShowcase {
    /// Arena layout:
    ///   0  text "Hey "
    ///   1  mention(SHOWCASE_PUBKEY_HEX)
    ///   2  text ", how are you?"
    ///   3  paragraph(children: [0, 1, 2])
    static var note: ContentTreeWire {
        ContentTreeWire(
            nodes: [
                .text("Hey "),
                .mention(
                    NostrWireUri(
                        uri: "nostr:\(SHOWCASE_NPUB)",
                        kind: .profile,
                        primaryId: SHOWCASE_PUBKEY_HEX
                    )
                ),
                .text(", how are you?"),
                .paragraph(children: [0, 1, 2]),
            ],
            roots: [3],
            mode: nil
        )
    }
}

struct ContentMentionChipPage: View {
    @Environment(GalleryModel.self) private var model
    @State private var rawMode = false

    var body: some View {
        let profile = model.profile(forPubkey: SHOWCASE_PUBKEY_HEX)
        VStack(spacing: 16) {
            // A hidden avatar owns the claim for the showcase pubkey while this
            // page is visible — same mechanism as `ProfileEmbedPage`, so the
            // mention chip resolves the display name instead of the hex fallback.
            NostrAvatar(pubkey: SHOWCASE_PUBKEY_HEX, size: 0)
                .frame(width: 0, height: 0)
                .clipped()
            RawToggle(rawMode: $rawMode)
            ContentPageFrame(caption: "NostrContentView — live mention resolution") {
                NostrContentView(
                    tree: MentionShowcase.note,
                    mentionLabel: { uri in
                        rawMode
                            ? uri.uri
                            : model.profile(forPubkey: uri.primaryId)?.displayName
                                ?? NostrContentView.defaultMentionLabel(uri)
                    }
                )
                Text("The kernel fetches kind:0 automatically; the app just reads the snapshot.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.top, 4)
            }
            ContentPageFrame(caption: "NostrMentionChip — kernel-backed profile") {
                NostrMentionChip(
                    pubkey: SHOWCASE_PUBKEY_HEX,
                    displayName: profile?.displayName,
                    avatarUrl: profile?.avatarURL
                )
            }
            ContentPageFrame(caption: "NostrMentionChip — reference fallback") {
                NostrMentionChip(
                    pubkey: SHOWCASE_PUBKEY_HEX,
                    displayName: nil
                )
            }
            ContentPageFrame(caption: "NostrMentionChip — no avatar") {
                NostrMentionChip(
                    pubkey: SHOWCASE_PUBKEY_HEX,
                    displayName: profile?.displayName,
                    showsAvatar: false
                )
            }
        }
    }
}

// MARK: - content-minimal

struct ContentMinimalPage: View {
    @Environment(GalleryModel.self) private var model
    @State private var rawMode = false

    var body: some View {
        VStack(spacing: 16) {
            // A hidden avatar owns the claim for the showcase pubkey while this
            // page is visible — same mechanism as `ProfileEmbedPage`, so the
            // minimal-run mention resolves the display name, not the hex fallback.
            NostrAvatar(pubkey: SHOWCASE_PUBKEY_HEX, size: 0)
                .frame(width: 0, height: 0)
                .clipped()
            RawToggle(rawMode: $rawMode)
            ContentPageFrame(caption: "NostrMinimalContentView(runs:)") {
                NostrMinimalContentView(runs: runs)
            }
        }
    }

    private var runs: [NostrContentRun] {
        ShowcaseContent.richTree.nostrMinimalRuns(
            mentionLabel: rawMode
                ? { uri in uri.uri }
                : { uri in
                    model.profile(forPubkey: uri.primaryId)?.displayName
                        ?? NostrContentView.defaultMentionLabel(uri)
                }
        )
    }
}

// MARK: - content-media-grid

struct ContentMediaGridPage: View {
    @Environment(GalleryModel.self) private var model

    var body: some View {
        let imageUrls = relayMediaUrls(from: model.embedHost)
        VStack(spacing: 16) {
            ContentPageFrame(caption: "NostrMediaGrid — relay-backed article media") {
                if imageUrls.isEmpty {
                    Text("Waiting for relay-backed media from the claimed article.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    NostrMediaGrid(imageUrls: imageUrls)
                }
            }
        }
        .task(id: SHOWCASE_ARTICLE_NADDR) {
            model.embedClaimSink.claim(
                uri: SHOWCASE_ARTICLE_NADDR,
                consumerId: "nmp-gallery-ios.content-media-grid"
            )
        }
        .onDisappear {
            model.embedClaimSink.release(
                uri: SHOWCASE_ARTICLE_NADDR,
                consumerId: "nmp-gallery-ios.content-media-grid"
            )
        }
    }
}

// MARK: - content-quote-card

struct ContentQuoteCardPage: View {
    @Environment(GalleryModel.self) private var model

    var body: some View {
        let quoteModel = relayQuoteModel(from: model)
        VStack(spacing: 16) {
            // The `.task` below claims the quote-card *event* (kind:1 content).
            // The author display name is a separate profile-host claim — owned
            // here by a hidden avatar, mirroring `ProfileEmbedPage`. The author
            // pubkey is statically `SHOWCASE_PUBKEY_HEX`; we claim it directly
            // rather than waiting on the resolved `note.authorPubkey`.
            NostrAvatar(pubkey: SHOWCASE_PUBKEY_HEX, size: 0)
                .frame(width: 0, height: 0)
                .clipped()
            ContentPageFrame(caption: "NostrQuoteCard — rich") {
                NostrQuoteCard(
                    model: quoteModel,
                    variant: .rich
                )
            }
            ContentPageFrame(caption: "NostrQuoteCard — compact") {
                NostrQuoteCard(
                    model: quoteModel,
                    variant: .compact
                )
            }
            ContentPageFrame(caption: "NostrQuoteCard — collapsed") {
                NostrQuoteCard(
                    model: quoteModel,
                    variant: .collapsed
                )
            }
            ContentPageFrame(caption: "NostrQuoteCard — missing") {
                NostrQuoteCard(
                    model: NostrQuoteCardModel(
                        id: SHOWCASE_NOTE_EVENT_ID,
                        unresolvedUri: SHOWCASE_NOTE_NEVENT
                    ),
                    variant: .missing
                )
            }
        }
        .task(id: SHOWCASE_NOTE_NEVENT) {
            model.embedClaimSink.claim(
                uri: SHOWCASE_NOTE_NEVENT,
                consumerId: "nmp-gallery-ios.content-quote-card"
            )
        }
        .onDisappear {
            model.embedClaimSink.release(
                uri: SHOWCASE_NOTE_NEVENT,
                consumerId: "nmp-gallery-ios.content-quote-card"
            )
        }
    }
}

private let showcaseNoteURI = NostrWireUri(
    uri: SHOWCASE_NOTE_NEVENT,
    kind: .event,
    primaryId: SHOWCASE_NOTE_EVENT_ID
)

@MainActor
private func relayMediaUrls(from host: EmbedHost) -> [URL] {
    var values: [String] = []
    if let article = host.envelopeForPrimaryID(SHOWCASE_ARTICLE_PRIMARY_ID),
       case .article(let projection) = article.projection,
       let hero = projection.heroImageUrl {
        values.append(hero)
    }
    if let note = host.envelopeForPrimaryID(SHOWCASE_NOTE_EVENT_ID),
       case .shortNote(let projection) = note.projection {
        values.append(contentsOf: projection.mediaUrls)
    }
    return values.compactMap(URL.init(string:))
}

@MainActor
private func relayQuoteModel(from model: GalleryModel) -> NostrQuoteCardModel {
    guard let envelope = model.embedHost.envelopeForPrimaryID(SHOWCASE_NOTE_EVENT_ID),
          case .shortNote(let note) = envelope.projection
    else {
        return NostrQuoteCardModel(
            id: SHOWCASE_NOTE_EVENT_ID,
            unresolvedUri: SHOWCASE_NOTE_NEVENT
        )
    }
    let profile = model.profile(forPubkey: note.authorPubkey)
    return NostrQuoteCardModel(
        id: note.id,
        unresolvedUri: SHOWCASE_NOTE_NEVENT,
        authorPubkey: note.authorPubkey,
        authorDisplayName: note.authorDisplayName ?? profile?.displayName,
        authorAvatarUrl: (note.authorPictureUrl ?? profile?.pictureUrl).flatMap(URL.init(string:)),
        content: note.content,
        mediaThumbnailUrl: note.mediaUrls.first.flatMap(URL.init(string:)),
        createdAtDisplay: note.createdAt == 0 ? nil : NostrRelativeTime.ago(note.createdAt)
    )
}
