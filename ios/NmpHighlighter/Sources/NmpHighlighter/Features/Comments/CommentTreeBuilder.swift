import Foundation

/// One node of a NIP-22 comment thread — a `CommentRecord` plus its
/// recursively built replies, sorted oldest-first within each level so
/// reading flows top-to-bottom chronologically.
struct CommentNode: Identifiable, Hashable {
    let record: CommentRecord
    var children: [CommentNode]

    var id: String { record.eventId }

    static func == (lhs: CommentNode, rhs: CommentNode) -> Bool {
        lhs.record.eventId == rhs.record.eventId
            && lhs.children.count == rhs.children.count
            && zip(lhs.children, rhs.children).allSatisfy { $0 == $1 }
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(record.eventId)
        hasher.combine(children.count)
    }
}

enum CommentTreeBuilder {
    /// Build a nested forest from a flat `[CommentRecord]`.
    ///
    /// Top-level comments are those whose lowercase parent tag mirrors
    /// the root scope (`parentTagValue == rootTagValue`). Replies are
    /// linked via `parentTagValue == <some_comment.eventId>`.
    ///
    /// Children are sorted ascending by `createdAt`; orphans (parent
    /// resolves to neither root nor a sibling in the input) are
    /// promoted to top-level so nothing is silently dropped.
    static func build(
        records: [CommentRecord],
        rootTagValue: String
    ) -> [CommentNode] {
        if records.isEmpty { return [] }

        // Sort once so child ordering naturally falls out chronological.
        let sorted = records.sorted { lhs, rhs in
            (lhs.createdAt ?? 0) < (rhs.createdAt ?? 0)
        }

        var byId: [String: [CommentRecord]] = [:]
        var seenIds = Set<String>()
        for r in sorted {
            byId[r.parentTagValue, default: []].append(r)
            seenIds.insert(r.eventId)
        }

        // Top-level: parent points at the root scope.
        var topLevel = byId[rootTagValue] ?? []

        // Promote orphans whose parent isn't the root and isn't a known
        // comment id — happens when a relay coughs up a reply but not
        // its parent. Surface them at top-level so the user can still
        // read them.
        for r in sorted {
            let parent = r.parentTagValue
            if parent == rootTagValue { continue }
            if seenIds.contains(parent) { continue }
            topLevel.append(r)
        }

        return topLevel.map { build(record: $0, byParent: byId) }
    }

    private static func build(
        record: CommentRecord,
        byParent: [String: [CommentRecord]]
    ) -> CommentNode {
        let children = (byParent[record.eventId] ?? []).map {
            build(record: $0, byParent: byParent)
        }
        return CommentNode(record: record, children: children)
    }
}

extension CommentNode {
    /// Total comment count under this node, inclusive of self.
    var totalCount: Int {
        1 + children.reduce(0) { $0 + $1.totalCount }
    }

    /// Most-recent reply (chronologically last child) — used for the
    /// inline depth-1 preview in the sheet root list. `nil` when the
    /// node has no replies.
    var mostRecentReply: CommentNode? {
        children.last
    }
}
