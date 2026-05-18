import SwiftUI

struct SettingsView: View {
    @Bindable var settings = Settings.shared
    @State private var showingClearCacheAlert = false
    @State private var showingClearSearchIndexAlert = false
    @State private var isClearing = false

    var body: some View {
        NavigationStack {
            List {
                playbackSection
                downloadsSection
                processingSection
                storageSection
                aboutSection
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
        }
    }

    // MARK: - Playback

    private var playbackSection: some View {
        Section {
            Toggle("Skip Ads", isOn: $settings.skipAds)

            Picker("Skip Forward", selection: $settings.skipForwardSeconds) {
                ForEach(Settings.skipForwardOptions, id: \.self) { seconds in
                    Text("\(Int(seconds))s").tag(seconds)
                }
            }

            Picker("Skip Backward", selection: $settings.skipBackwardSeconds) {
                ForEach(Settings.skipBackwardOptions, id: \.self) { seconds in
                    Text("\(Int(seconds))s").tag(seconds)
                }
            }

            Picker("Default Speed", selection: $settings.defaultPlaybackRate) {
                ForEach(Settings.playbackRateOptions, id: \.self) { rate in
                    Text(formatRate(rate)).tag(rate)
                }
            }
        } header: {
            Text("Playback")
        } footer: {
            Text("When Skip Ads is enabled, AI-detected sponsor segments will be automatically skipped.")
        }
    }

    private func formatRate(_ rate: Float) -> String {
        if rate == 1.0 {
            return "1x"
        } else if rate.truncatingRemainder(dividingBy: 1) == 0 {
            return "\(Int(rate))x"
        } else {
            return "\(rate)x"
        }
    }

    // MARK: - Downloads

    private var downloadsSection: some View {
        Section("Downloads") {
            Toggle("Allow Cellular Downloads", isOn: $settings.allowCellularDownloads)
        }
    }

    // MARK: - Processing

    private var processingSection: some View {
        Section {
            Toggle("Auto-Transcribe", isOn: $settings.autoTranscribe)
            Toggle("Auto-Summarize", isOn: $settings.autoSummarize)
            Toggle("Auto-Extract Chapters", isOn: $settings.autoExtractChapters)

            Picker("Summary Style", selection: $settings.defaultSummaryStyle) {
                ForEach(SummaryStyle.allCases, id: \.self) { style in
                    Text(style.displayName).tag(style)
                }
            }
        } header: {
            Text("Processing")
        } footer: {
            Text("When enabled, downloaded episodes will automatically be processed by AI.")
        }
    }

    // MARK: - Storage

    private var storageSection: some View {
        Section("Storage") {
            Button(role: .destructive) {
                showingClearCacheAlert = true
            } label: {
                HStack {
                    Text("Clear Image Cache")
                    Spacer()
                    if isClearing {
                        ProgressView()
                    }
                }
            }
            .disabled(isClearing)
            .alert("Clear Image Cache?", isPresented: $showingClearCacheAlert) {
                Button("Cancel", role: .cancel) {}
                Button("Clear", role: .destructive) {
                    Task {
                        isClearing = true
                        await ImageCache.shared.clearCache()
                        isClearing = false
                    }
                }
            } message: {
                Text("This will remove all cached podcast artwork. Images will be re-downloaded as needed.")
            }

            Button(role: .destructive) {
                showingClearSearchIndexAlert = true
            } label: {
                Text("Clear Search Index")
            }
            .disabled(isClearing)
            .alert("Clear Search Index?", isPresented: $showingClearSearchIndexAlert) {
                Button("Cancel", role: .cancel) {}
                Button("Clear", role: .destructive) {
                    Task {
                        isClearing = true
                        do {
                            try await ServiceContainer.shared.vectorDatabase.clearAllVectors()
                        } catch {
                            // Silently fail - user can retry
                        }
                        isClearing = false
                    }
                }
            } message: {
                Text("This will remove the search index. Transcripts will need to be re-indexed for AI search to work.")
            }
        }
    }

    // MARK: - About

    private var aboutSection: some View {
        Section("About") {
            HStack {
                Text("Version")
                Spacer()
                Text(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0")
                    .foregroundStyle(.secondary)
            }

            HStack {
                Text("Build")
                Spacer()
                Text(Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "1")
                    .foregroundStyle(.secondary)
            }
        }
    }
}

#Preview {
    SettingsView()
}
