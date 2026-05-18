import SwiftUI

// MARK: - SettingsView
//
// T-podcast-gap-002: Verbatim Podcastr SettingsView requires AppStateStore
// backed settings. Stub until kernel exposes settings store.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Settings/SettingsView.swift

struct SettingsView: View {
    var body: some View {
        ContentUnavailableView(
            "Settings",
            systemImage: "gear",
            description: Text("App settings load once the kernel exposes the settings store (T-podcast-gap-002).")
        )
        .navigationTitle("Settings")
        .navigationBarTitleDisplayMode(.inline)
    }
}
