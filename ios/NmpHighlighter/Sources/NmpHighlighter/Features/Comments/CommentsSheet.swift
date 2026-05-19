import SwiftUI

/// Root comments screen pushed onto the enclosing NavigationStack.
/// No inner NavigationStack — thread drill-down is handled by
/// ThreadView's own `.navigationDestination(item:)`.
struct CommentsView: View {
    let artifact: ArtifactRef
    let artifactAuthorPubkey: String?
    let artifactHeader: AnyView?
    let store: CommentsStore

    var body: some View {
        ThreadView(
            focused: nil,
            artifactHeader: artifactHeader,
            store: store,
            artifact: artifact,
            artifactAuthorPubkey: artifactAuthorPubkey
        )
    }
}
