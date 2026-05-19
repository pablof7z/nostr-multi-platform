import SwiftUI

/// Attaches NIP-22 comments to any reader by injecting a top-bar toolbar
/// button (bubble icon + count) that pushes a CommentsView onto the
/// enclosing NavigationStack. Owns the CommentsStore lifecycle so the
/// count is live before the user ever taps.
struct CommentsAttachment: ViewModifier {
    let artifact: ArtifactRef
    let artifactAuthorPubkey: String?
    let artifactHeader: AnyView?

    @Environment(HighlighterStore.self) private var app
    @State private var store = CommentsStore()
    @State private var showComments = false
    @State private var didStart = false

    func body(content: Content) -> some View {
        content
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { showComments = true } label: {
                        commentsLabel
                    }
                    .accessibilityLabel(
                        store.totalCount == 0
                            ? "Start the thread"
                            : "\(store.totalCount) comments"
                    )
                }
            }
            .navigationDestination(isPresented: $showComments) {
                CommentsView(
                    artifact: artifact,
                    artifactAuthorPubkey: artifactAuthorPubkey,
                    artifactHeader: artifactHeader,
                    store: store
                )
            }
            .task(id: artifact) {
                guard !didStart else { return }
                didStart = true
                await store.start(
                    artifact: artifact,
                    core: app.safeCore,
                    currentUserPubkey: app.currentUser?.pubkey
                )
            }
    }

    private var commentsLabel: some View {
        HStack(spacing: 4) {
            Image(systemName: "bubble.left")
                .font(.system(size: 15, weight: .medium))
            if store.totalCount > 0 {
                Text("\(store.totalCount)")
                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                    .monospacedDigit()
            }
        }
    }
}

extension View {
    func commentsAttachment(
        artifact: ArtifactRef,
        artifactAuthorPubkey: String? = nil,
        artifactHeader: AnyView? = nil
    ) -> some View {
        modifier(CommentsAttachment(
            artifact: artifact,
            artifactAuthorPubkey: artifactAuthorPubkey,
            artifactHeader: artifactHeader
        ))
    }
}
