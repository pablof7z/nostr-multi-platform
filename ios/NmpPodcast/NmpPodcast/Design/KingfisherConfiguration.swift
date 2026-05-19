import Foundation

// MARK: - KingfisherConfiguration
//
// T-podcast-ios-RESTART: Shim — NmpPodcast uses AsyncImage (not Kingfisher) as
// its image backend. AppDelegate calls KingfisherConfiguration.configure() on
// cold launch; this shim makes it a no-op.
//
// When Kingfisher is added as an SPM dep (T-podcast-gap-005), replace this
// file byte-for-byte with:
// /Users/pablofernandez/Work/podcast/App/Sources/Design/KingfisherConfiguration.swift

enum KingfisherConfiguration {
    static func configure() {
        // No-op: Kingfisher not linked yet. AppDelegate.configure() call kept
        // verbatim; this shim satisfies the symbol requirement.
    }
}
