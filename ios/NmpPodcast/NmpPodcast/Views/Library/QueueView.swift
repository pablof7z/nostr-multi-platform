import SwiftUI

struct QueueView: View {
    @Bindable var processingQueue: ProcessingQueue
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                if processingQueue.activeJobs.isEmpty && processingQueue.completedJobs.isEmpty && processingQueue.failedJobs.isEmpty {
                    ContentUnavailableView(
                        "No Downloads",
                        systemImage: "arrow.down.circle",
                        description: Text("Downloads will appear here when you play episodes.")
                    )
                } else {
                    if !processingQueue.activeJobs.isEmpty {
                        Section("Active") {
                            ForEach(processingQueue.activeJobs) { job in
                                QueueJobRow(job: job, onCancel: {
                                    processingQueue.cancelJob(job)
                                })
                            }
                        }
                    }

                    if !processingQueue.completedJobs.isEmpty {
                        Section {
                            ForEach(processingQueue.completedJobs) { job in
                                QueueJobRow(job: job, onCancel: nil)
                            }
                        } header: {
                            HStack {
                                Text("Completed")
                                Spacer()
                                Button("Clear") {
                                    processingQueue.clearCompleted()
                                }
                                .font(.caption)
                            }
                        }
                    }

                    if !processingQueue.failedJobs.isEmpty {
                        Section {
                            ForEach(processingQueue.failedJobs) { job in
                                QueueJobRow(job: job, onCancel: nil)
                            }
                        } header: {
                            HStack {
                                Text("Failed")
                                Spacer()
                                Button("Clear") {
                                    processingQueue.clearFailed()
                                }
                                .font(.caption)
                            }
                        }
                    }
                }
            }
            .navigationTitle("Downloads")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
    }
}

struct QueueJobRow: View {
    let job: QueueJob
    var onCancel: (() -> Void)?

    var body: some View {
        HStack(spacing: 12) {
            statusIcon

            VStack(alignment: .leading, spacing: 4) {
                Text(job.episodeTitle)
                    .font(.subheadline)
                    .lineLimit(2)

                Text(job.type.rawValue)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            if let onCancel = onCancel, job.status == .running || job.status == .queued {
                Button {
                    onCancel()
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch job.status {
        case .queued:
            Image(systemName: "clock")
                .foregroundStyle(.secondary)
        case .running:
            ProgressView()
                .scaleEffect(0.8)
        case .completed:
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
        case .failed:
            Image(systemName: "exclamationmark.circle.fill")
                .foregroundStyle(.red)
        }
    }
}

#Preview {
    QueueView(processingQueue: ServiceContainer.shared.processingQueue)
}
