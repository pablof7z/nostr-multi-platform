import SwiftUI
import AVFoundation
import OSLog
import UIKit

struct PlayerSheet: View {
    @Bindable var audioService: AudioService
    var insightService: InsightService
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext

    @State private var selectedGuest: Guest?
    @State private var isCapturing = false
    @State private var captureTime: TimeInterval = 0
    @State private var showInsightSavedToast = false
    @State private var showInsightErrorToast = false
    @State private var insightErrorMessage = ""
    @State private var pulseAnimation = false
    @State private var dragOffset: CGFloat = 0

    private let processingQueue = ServiceContainer.shared.processingQueue
    private let aiService = ServiceContainer.shared.aiService

    var body: some View {
        if let episode = audioService.currentEpisode {
            GeometryReader { geometry in
                ZStack {
                    // Main player content
                    ZStack {
                        // Dynamic background
                        dynamicBackground(for: episode)

                        VStack(spacing: 0) {
                            // Drag indicator
                            dragIndicator

                            // Compact header
                            headerSection(episode: episode)

                            // Scrollable content area
                            ScrollView {
                                VStack(spacing: 16) {
                                    // AI Summary
                                    summarySection(episode: episode)

                                    // Chapters
                                    chaptersSection(episode: episode)
                                }
                                .padding(.horizontal)
                                .padding(.vertical, 12)
                            }

                            Spacer(minLength: 0)

                            // Playback controls bar
                            controlsBar(episode: episode)
                        }
                        .safeAreaPadding(.top)
                        .safeAreaPadding(.bottom)
                    }
                    .frame(width: geometry.size.width, height: geometry.size.height)
                    .clipped()
                }
                .overlay(alignment: .top) {
                    toastOverlays
                }
                .offset(y: dragOffset)
                .gesture(dismissGesture(height: geometry.size.height))
            }
            .sheet(item: $selectedGuest) { guest in
                GuestAgentSheet(guest: guest, episode: episode)
            }
        }
    }

    // MARK: - Background

    func dynamicBackground(for episode: Episode) -> some View {
        ZStack {
            Color(.systemBackground)

            if let artworkURL = episode.podcast?.artworkURL {
                CachedAsyncImage(url: artworkURL) {
                    Color.clear
                }
                .aspectRatio(contentMode: .fill)
                .blur(radius: 80)
                .opacity(0.3)
                .ignoresSafeArea()
            }
        }
        .ignoresSafeArea()
    }

    // MARK: - Drag Indicator

    var dragIndicator: some View {
        Capsule()
            .fill(Color.secondary.opacity(0.4))
            .frame(width: 36, height: 5)
            .padding(.top, 8)
            .padding(.bottom, 4)
    }

    // MARK: - Header

    func headerSection(episode: Episode) -> some View {
        HStack(spacing: 12) {
            // Artwork
            CachedAsyncImage(url: episode.podcast?.artworkURL) {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.secondary.opacity(0.2))
                    .overlay {
                        Image(systemName: "waveform")
                            .font(.title3)
                            .foregroundStyle(.secondary)
                    }
            }
            .aspectRatio(contentMode: .fill)
            .frame(width: 56, height: 56)
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .shadow(color: .black.opacity(0.15), radius: 4, x: 0, y: 2)

            // Episode info
            VStack(alignment: .leading, spacing: 2) {
                Text(episode.title)
                    .font(.subheadline)
                    .fontWeight(.semibold)
                    .lineLimit(2)

                if let podcastTitle = episode.podcast?.title {
                    Text(podcastTitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }

            Spacer()

            // Guest buttons
            ForEach(episode.guests.prefix(2)) { guest in
                Button {
                    selectedGuest = guest
                } label: {
                    Image(systemName: "person.circle.fill")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                }
                .accessibilityLabel("Ask \(guest.name)")
                .accessibilityIdentifier("askGuestButton")
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
    }

    // MARK: - Summary Section

    func summarySection(episode: Episode) -> some View {
        Group {
            if let summary = episode.aiSummary {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Image(systemName: "sparkles")
                            .foregroundStyle(.purple)
                        Text("AI Summary")
                            .font(.subheadline)
                            .fontWeight(.semibold)
                    }
                    .foregroundStyle(.primary)

                    Text(summary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineSpacing(4)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding()
                .background(Color.purple.opacity(0.08))
                .clipShape(RoundedRectangle(cornerRadius: 12))
            }
        }
    }

    // MARK: - Chapters Section

    var sortedChapters: [Chapter] {
        audioService.currentEpisode?.transcript?.chapters.sorted { $0.chapterIndex < $1.chapterIndex } ?? []
    }

    var currentChapterIndex: Int? {
        let currentTime = audioService.currentTime
        return sortedChapters.firstIndex { chapter in
            currentTime >= chapter.startTime && currentTime < chapter.endTime
        }
    }

    var isExtractingChapters: Bool {
        guard let episode = audioService.currentEpisode else { return false }
        return processingQueue.jobs.contains { job in
            job.episodeID == episode.id &&
            job.type == .extractChapters &&
            (job.status == .queued || job.status == .running)
        }
    }

    func chaptersSection(episode: Episode) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "list.bullet")
                    .foregroundStyle(.blue)
                Text("Chapters")
                    .font(.subheadline)
                    .fontWeight(.semibold)
            }
            .foregroundStyle(.primary)

            if sortedChapters.isEmpty {
                chaptersEmptyState(episode: episode)
            } else {
                chaptersContent
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    func chaptersEmptyState(episode: Episode) -> some View {
        VStack(spacing: 12) {
            if isExtractingChapters {
                HStack(spacing: 8) {
                    ProgressView()
                        .scaleEffect(0.9)
                    Text("Generating chapters...")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            } else if episode.transcript == nil {
                Text("Transcript required to generate chapters")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                Text("No chapters available")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Button {
                    processingQueue.enqueueChapterExtraction(episode: episode)
                } label: {
                    Label("Generate Chapters", systemImage: "sparkles")
                        .font(.caption)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 20)
    }

    var chaptersContent: some View {
        LazyVStack(spacing: 0) {
            ForEach(Array(sortedChapters.enumerated()), id: \.element.id) { index, chapter in
                ChapterRow(
                    chapter: chapter,
                    isActive: index == currentChapterIndex,
                    onTap: {
                        audioService.seek(to: chapter.startTime)
                    }
                )
            }
        }
    }
}
