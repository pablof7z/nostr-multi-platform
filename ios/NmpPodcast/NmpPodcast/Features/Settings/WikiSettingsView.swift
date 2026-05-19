import SwiftUI

// MARK: - WikiSettingsView
//
// Settings → Wiki. Controls wiki automation. The compilation model is selected
// under Settings → Intelligence → Models → Wiki.

struct WikiSettingsView: View {
    var body: some View {
        Form {
            automationSection
        }
        .navigationTitle("Wiki")
        .navigationBarTitleDisplayMode(.inline)
    }

    // MARK: - Sections

    private var automationSection: some View {
        Section {
            Label("Manual refresh", systemImage: "arrow.triangle.2.circlepath")
        } footer: {
            Text("Automatic transcript-triggered wiki refresh is not active in this build. Refresh a page from the Wiki tab after new transcripts finish ingesting; choose the compilation model in Models → Wiki.")
        }
    }
}
