import SwiftUI

// MARK: - CategoriesRecomputeSheet
//
// Modal flow that triggers `PodcastCategorizationService.recompute(store:)`
// and surfaces progress + the resulting categories. Presented from
// `CategoriesListView` so category management stays inside Library.

struct CategoriesRecomputeSheet: View {
    @Environment(AppStateStore.self) private var store
    @Environment(\.dismiss) private var dismiss

    @State private var service = PodcastCategorizationService.shared
    @State private var phase: Phase = .idle
    @State private var errorMessage: String?

    private enum Phase: Equatable {
        case idle
        case running
        case finished(count: Int)
        case failed
    }

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("Categories")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Close") { dismiss() }
                    }
                }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .onAppear {
            seedPhase()
        }
    }

    @ViewBuilder
    private var content: some View {
        switch phase {
        case .idle:
            idleView
        case .running:
            runningView
        case .finished:
            resultsList
        case .failed:
            failureView
        }
    }

    // MARK: - States

    private var idleView: some View {
        List {
            Section {
                summaryRow
                Button {
                    Task { await runRecompute() }
                } label: {
                    Label("Recompute Categories", systemImage: "wand.and.sparkles")
                }
                .disabled(store.state.subscriptions.isEmpty)
            } footer: {
                Text("Asks the configured AI model to group every podcast you follow into 6-12 categories. Existing categories are replaced.")
            }
        }
    }

    private var runningView: some View {
        VStack(spacing: 16) {
            ProgressView()
                .controlSize(.large)
            Text("Generating categories…")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }

    private var resultsList: some View {
        List {
            Section {
                ForEach(store.state.categories) { category in
                    categoryRow(category)
                }
            } header: {
                Text("\(store.state.categories.count) categories")
            } footer: {
                if let lastRun = service.lastRun {
                    Text("Generated \(lastRun.formatted(.relative(presentation: .named))).")
                }
            }

            Section {
                Button {
                    Task { await runRecompute() }
                } label: {
                    Label("Recompute Again", systemImage: "arrow.clockwise")
                }
            }
        }
    }

    private var failureView: some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 36, weight: .semibold))
                .foregroundStyle(AppTheme.Tint.warning)
            Text("Couldn't generate categories")
                .font(AppTheme.Typography.title3)
            if let errorMessage {
                Text(errorMessage)
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal)
            }
            Button {
                Task { await runRecompute() }
            } label: {
                Label("Try Again", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.borderedProminent)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }

    // MARK: - Sub-views

    @ViewBuilder
    private var summaryRow: some View {
        let count = store.state.subscriptions.count
        if count == 0 {
            Text("Add at least one podcast subscription first.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.secondary)
        } else {
            HStack {
                Text("Subscriptions")
                Spacer()
                Text("\(count)")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func categoryRow(_ category: PodcastCategory) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(category.name)
                    .font(AppTheme.Typography.body)
                Spacer()
                Text("\(category.subscriptionIDs.count)")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
            }
            if !category.description.isEmpty {
                Text(category.description)
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 2)
    }

    // MARK: - Actions

    private func seedPhase() {
        if service.isRunning {
            phase = .running
        } else if !store.state.categories.isEmpty {
            phase = .finished(count: store.state.categories.count)
        } else {
            phase = .idle
        }
    }

    private func runRecompute() async {
        errorMessage = nil
        phase = .running
        do {
            try await service.recompute(store: store)
            phase = .finished(count: store.state.categories.count)
            Haptics.success()
        } catch let error as CategorizationError {
            errorMessage = error.errorDescription
            phase = .failed
            Haptics.error()
        } catch {
            errorMessage = error.localizedDescription
            phase = .failed
            Haptics.error()
        }
    }
}
