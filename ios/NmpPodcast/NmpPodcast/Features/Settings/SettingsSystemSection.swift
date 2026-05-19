import SwiftUI

struct SettingsSystemSection: View {
    let notificationsRowValue: String?
    let networkingRowValue: String
    let dataStorageSummary: String?

    var body: some View {
        Section("System") {
            NavigationLink {
                NotificationSettingsView()
            } label: {
                SettingsRow(
                    icon: "bell.badge",
                    tint: .red,
                    title: "Notifications",
                    value: notificationsRowValue
                )
            }

            NavigationLink {
                NetworkingSettingsView()
            } label: {
                SettingsRow(
                    icon: "network",
                    tint: .blue,
                    title: "Networking",
                    value: networkingRowValue
                )
            }

            NavigationLink {
                DataStorageSettingsView()
            } label: {
                SettingsRow(
                    icon: "externaldrive.fill",
                    tint: .teal,
                    title: "Data & Storage",
                    value: dataStorageSummary
                )
            }
        }
    }
}
