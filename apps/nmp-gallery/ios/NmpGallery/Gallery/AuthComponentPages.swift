import SwiftUI

// MARK: - login-block

/// Renders the `NostrLoginBlock` registry component.
///
/// The block probes for installed Nostr signer apps (Amber, Primal,
/// nostrconnect) lazily in `.task {}` via `UIApplication.canOpenURL`. The
/// gallery's Info.plist declares no `LSApplicationQueriesSchemes`, and the
/// simulator has no signer apps installed, so the probe returns an empty list
/// — the correct degraded state. The block then shows the always-present
/// manual key-entry option plus the "install a signer" hint.
///
/// The page passes no-op closures: this is a visual showcase, not a live
/// sign-in flow. Tapping a card / the manual button does nothing.
struct LoginBlockPage: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("NostrLoginBlock(onSignerSelected:onManualKey:)")
                .font(.caption)
                .foregroundStyle(.secondary)
            VStack {
                NostrLoginBlock(
                    onSignerSelected: { _ in },
                    onManualKey: {}
                )
            }
            .frame(maxWidth: .infinity)
            .padding(20)
            .background(Color(.secondarySystemGroupedBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }
}
