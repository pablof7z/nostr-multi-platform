import Foundation

// Per-open flat-feed accessors (author / thread / hashtag) plus the go-to-box
// query classifier. Split out of `KernelModel.swift` to keep it under the
// file-size hard cap; these are thin pass-throughs to `KernelHandle` and reads
// of the dynamic `flatFeeds` projection dictionary (keyed `nmp.feed.<type>.<id>`).
extension KernelModel {
    // ── View / Author / Thread open + close ──────────────────────────────
    func openAuthor(pubkey: String) { kernel.openAuthor(pubkey: pubkey) }
    func closeAuthor(pubkey: String) { kernel.closeAuthor(pubkey: pubkey) }
    func openThread(eventID: String) { kernel.openThread(eventID: eventID) }
    func closeThread(eventID: String) { kernel.closeThread(eventID: eventID) }
    func authorFeed(pubkey: String) -> OpFeedSnapshot? {
        flatFeeds["nmp.feed.author.\(pubkey)"]
    }
    func threadFeed(eventID: String) -> OpFeedSnapshot? {
        flatFeeds["nmp.feed.thread.\(eventID)"]
    }

    // ── Hashtag feed open + close ────────────────────────────────────────
    func openTag(tag: String) { kernel.openTag(tag: tag) }
    func closeTag(tag: String) { kernel.closeTag(tag: tag) }
    func tagFeed(tag: String) -> OpFeedSnapshot? {
        flatFeeds["nmp.feed.tag.\(tag)"]
    }

    /// Classify a raw go-to-box query via the Rust thin-shell classifier.
    func classify(query: String) -> SearchClassification { kernel.classify(query: query) }
}
