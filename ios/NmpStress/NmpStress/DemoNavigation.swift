import SwiftUI

enum DemoView: Hashable {
    case timeline
    case diagnostics

    var title: String {
        switch self {
        case .timeline:
            "Timeline"
        case .diagnostics:
            "Diagnostics"
        }
    }
}

enum DemoRoute: Hashable, Identifiable {
    case author(String)
    case thread(String)

    var id: String {
        switch self {
        case let .author(pubkey):
            "author-\(pubkey)"
        case let .thread(eventID):
            "thread-\(eventID)"
        }
    }
}
