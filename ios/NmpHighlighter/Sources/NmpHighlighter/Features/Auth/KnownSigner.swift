import UIKit

/// Mirrors Olas iOS's `KnownSigner` enum
/// (`Olas-iOS-60m1gj/OlasApp/Views/Auth/LoginView.swift:29-57`).
///
/// Detection is by URL scheme only, via `UIApplication.canOpenURL`. The
/// schemes must also be declared in Info.plist's `LSApplicationQueriesSchemes`
/// or `canOpenURL` returns false even when the app is installed.
enum KnownSigner: CaseIterable {
    case amber
    case primal
    case other

    var name: String {
        switch self {
        case .amber: return "Amber"
        case .primal: return "Primal"
        case .other: return "Signer App"
        }
    }

    var urlScheme: String {
        switch self {
        case .amber: return "nostrsigner"
        case .primal: return "primal"
        case .other: return "nostrconnect"
        }
    }

    /// Probe installed signer apps in Olas's priority order (amber → primal →
    /// generic). Returns the most-specific match or `nil` if none detected.
    @MainActor
    static func detect() -> KnownSigner? {
        for signer in KnownSigner.allCases {
            if let url = URL(string: "\(signer.urlScheme)://"),
               UIApplication.shared.canOpenURL(url) {
                return signer
            }
        }
        return nil
    }
}
