import Foundation
import Observation

/// View-scoped reactive state for the NIP-22 comment thread on a single
/// artifact. Holds the flat record list, the built tree, per-comment
/// reaction + bookmark state, and the in-flight composer drafts.
///
/// Pattern follows `RoomStore` — allocated by the consuming view in a
/// `.task { }`, deallocated on disappear; reads NostrDB via the Rust
/// core; never fabricates data the core doesn't have.
@MainActor
@Observable
final class CommentsStore {
    private(set) var records: [CommentRecord] = []
    private(set) var tree: [CommentNode] = []
    private(set) var isLoading: Bool = true
    private(set) var loadError: String?

    /// kind:7 like counts for any visible comment id. Updated optimistically
    /// on toggle and reconciled on refresh.
    private(set) var likeCounts: [String: Int] = [:]
    /// Reaction event id for the current user's like on a given comment id
    /// (allows quick "undo like" via NIP-09 deletion). Missing = not liked.
    private(set) var myLikeEventIds: [String: String] = [:]
    /// Bookmark membership for any visible comment id.
    private(set) var bookmarked: Set<String> = []

    /// Drafts keyed by `parentEventId ?? "root"`. In-memory only — survives
    /// detent transitions but not view recreation. (Persistent drafts are
    /// deferred per design doc.)
    private(set) var drafts: [String: String] = [:]

    @ObservationIgnored private var artifact: ArtifactRef?
    @ObservationIgnored private var core: SafeHighlighterCore?
    @ObservationIgnored private var currentUserPubkey: String?

    // MARK: - Lifecycle

    func start(
        artifact: ArtifactRef,
        core: SafeHighlighterCore,
        currentUserPubkey: String?
    ) async {
        self.artifact = artifact
        self.core = core
        self.currentUserPubkey = currentUserPubkey
        await refresh()
    }

    func refresh() async {
        guard let core, let artifact else { return }
        isLoading = true
        loadError = nil
        do {
            let fetched = try await core.getCommentsForReference(
                tagName: artifact.rootTagName,
                tagValue: artifact.rootTagValue,
                limit: 256
            )
            records = fetched
            tree = CommentTreeBuilder.build(
                records: fetched,
                rootTagValue: artifact.rootTagValue
            )
            await refreshReactionsAndBookmarks(for: fetched)
        } catch {
            loadError = (error as? CoreError).map { "\($0)" } ?? "Couldn't load comments."
        }
        isLoading = false
    }

    /// Reaction counts + my-bookmark predicates for every visible comment.
    /// Runs in parallel; failures leave previous state in place.
    private func refreshReactionsAndBookmarks(for records: [CommentRecord]) async {
        guard let core else { return }
        let captured = core
        await withTaskGroup(of: (String, [ReactionRecord]?, Bool?).self) { group in
            for r in records {
                let id = r.eventId
                group.addTask {
                    let reactions = try? await captured.getReactionsForEvent(targetEventId: id, limit: 128)
                    let bookmarked = try? await captured.isEventBookmarked(eventIdHex: id)
                    return (id, reactions, bookmarked)
                }
            }
            for await (id, reactions, isBookmarked) in group {
                if let reactions {
                    let likes = reactions.filter { $0.content == "+" }
                    likeCounts[id] = likes.count
                    if let me = currentUserPubkey,
                       let mine = likes.first(where: { $0.pubkey == me }) {
                        myLikeEventIds[id] = mine.eventId
                    } else {
                        myLikeEventIds.removeValue(forKey: id)
                    }
                }
                if let isBookmarked {
                    if isBookmarked { self.bookmarked.insert(id) }
                    else { self.bookmarked.remove(id) }
                }
            }
        }
    }

    // MARK: - Drafts

    func draft(forParent parentId: String?) -> String {
        drafts[parentId ?? "root"] ?? ""
    }

    func setDraft(_ text: String, forParent parentId: String?) {
        let key = parentId ?? "root"
        if text.isEmpty {
            drafts.removeValue(forKey: key)
        } else {
            drafts[key] = text
        }
    }

    // MARK: - Publish

    /// Publish a comment scoped to the artifact. `parentEventId == nil`
    /// posts a top-level thread; otherwise posts as a reply to that
    /// kind:1111 comment. Optimistically inserts the new record and
    /// rebuilds the tree.
    @discardableResult
    func publish(content: String, parentEventId: String?) async throws -> CommentRecord {
        guard let core, let artifact else {
            throw CoreError.NotInitialized(message: "store not started")
        }
        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            throw CoreError.InvalidInput(message: "comment body must not be empty")
        }

        let record = try await core.publishComment(
            rootTagName: artifact.rootTagName,
            rootTagValue: artifact.rootTagValue,
            rootKind: artifact.rootKind,
            parentEventId: parentEventId,
            content: trimmed
        )

        // Optimistic insert
        if !records.contains(where: { $0.eventId == record.eventId }) {
            records.append(record)
            tree = CommentTreeBuilder.build(
                records: records,
                rootTagValue: artifact.rootTagValue
            )
        }
        setDraft("", forParent: parentEventId)
        return record
    }

    // MARK: - Like (kind:7)

    func isLiked(_ commentId: String) -> Bool {
        myLikeEventIds[commentId] != nil
    }

    func likeCount(_ commentId: String) -> Int {
        likeCounts[commentId] ?? 0
    }

    /// Toggle a like on `comment`. If the user already liked, deletes the
    /// reaction via NIP-09. Optimistic count + state update.
    func toggleLike(_ comment: CommentRecord) async {
        guard let core else { return }
        let id = comment.eventId
        let alreadyLiked = isLiked(id)

        // Optimistic
        let prevCount = likeCount(id)
        if alreadyLiked {
            likeCounts[id] = max(0, prevCount - 1)
        } else {
            likeCounts[id] = prevCount + 1
            myLikeEventIds[id] = "pending"
        }

        do {
            if alreadyLiked, let myReactionId = myLikeEventIds[id], myReactionId != "pending" {
                _ = try await core.unpublishReaction(reactionEventId: myReactionId)
                myLikeEventIds.removeValue(forKey: id)
            } else {
                let kind = UInt16(1111)
                let reaction = try await core.publishReaction(
                    eventId: id,
                    authorPubkeyHex: comment.pubkey,
                    targetKind: kind,
                    content: "+"
                )
                myLikeEventIds[id] = reaction.eventId
            }
        } catch {
            // Roll back on failure
            likeCounts[id] = prevCount
            if alreadyLiked {
                // (we already had a reaction id; restore it if we still know it)
                // Best effort: leave count restored.
            } else {
                myLikeEventIds.removeValue(forKey: id)
            }
        }
    }

    // MARK: - Bookmark (kind:10003)

    func isBookmarked(_ commentId: String) -> Bool {
        bookmarked.contains(commentId)
    }

    func toggleBookmark(_ comment: CommentRecord) async {
        guard let core else { return }
        let id = comment.eventId
        let was = bookmarked.contains(id)
        if was { bookmarked.remove(id) } else { bookmarked.insert(id) }
        do {
            let now = try await core.toggleEventBookmark(eventIdHex: id)
            if now { bookmarked.insert(id) } else { bookmarked.remove(id) }
        } catch {
            // Roll back
            if was { bookmarked.insert(id) } else { bookmarked.remove(id) }
        }
    }
}

// MARK: - Convenience accessors

extension CommentsStore {
    /// Total comment count across the whole tree (top-level + replies).
    var totalCount: Int {
        records.count
    }

    /// The N most-recent commenter pubkeys (for the toolbar avatar trio).
    func recentCommenterPubkeys(limit: Int = 3) -> [String] {
        let sorted = records.sorted { ($0.createdAt ?? 0) > ($1.createdAt ?? 0) }
        var seen = Set<String>()
        var out: [String] = []
        for r in sorted {
            if seen.insert(r.pubkey).inserted {
                out.append(r.pubkey)
                if out.count >= limit { break }
            }
        }
        return out
    }
}
