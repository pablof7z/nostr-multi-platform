import SwiftUI

/// Surface the scope-deferred features honestly so QA + user know what is
/// stubbed vs unwired. Each item corresponds to a follow-up task.
struct PendingFeaturesView: View {
    var body: some View {
        List {
            Section {
                Text("Pulse v0 ships timeline-only. The five-screen Pulse design (Onboarding, Timeline, NoteDetail, Compose, Accounts) requires FFI surface extensions filed as T66a.")
                    .font(.body)
                    .foregroundStyle(.primary)
            } header: {
                Text("Scope")
            }

            Section("Filed for follow-up") {
                pendingRow(
                    title: "Onboarding (paste nsec / bunker)",
                    detail: "Needs nmp_app_signin_nsec + nmp_app_signin_bunker FFI commands + actor-side AccountManager wiring.",
                    task: "T66a"
                )
                pendingRow(
                    title: "Compose (publish kind:1)",
                    detail: "Needs nmp_app_publish_note FFI + PublishEngine wired into actor, with Nip65OutboxResolver as outbox.",
                    task: "T66a"
                )
                pendingRow(
                    title: "Accounts (multi-session switch)",
                    detail: "Needs ActiveAccountReactor bundle executed by actor (translator landed in nmp-signers/identity/active_account_reactor.rs).",
                    task: "T66a"
                )
                pendingRow(
                    title: "NoteDetail (replies + likes)",
                    detail: "Needs nmp_app_react FFI + reply-tree projection.",
                    task: "T66a"
                )
            }

            Section("Already substrate-complete") {
                Text("• Nip65OutboxResolver — kernel can resolve PublishTarget::Auto from kind:10002.")
                Text("• ActiveAccountReactor — observer + atomic command bundle for active-account transitions.")
                Text("• Real-relay smoke test — kind:1 round-trip verified against wss://relay.damus.io.")
                Text("• AccountManager — multi-signer + applesauce post-condition + observers.")
            }
            .font(.callout)
        }
        .navigationTitle("Status")
    }

    private func pendingRow(title: String, detail: String, task: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(title).font(.body).bold()
                Spacer()
                Text(task)
                    .font(.caption2)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor.opacity(0.15))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }
            Text(detail)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
    }
}
