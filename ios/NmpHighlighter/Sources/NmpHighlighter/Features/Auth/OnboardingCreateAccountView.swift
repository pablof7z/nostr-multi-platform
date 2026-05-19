import SwiftUI

private let nip05BaseURL = URL(string: "https://beta.highlighter.com")!

private enum UsernameState: Equatable {
    case idle
    case checking
    case available(identifier: String, domain: String)
    case taken
    case invalid
}

struct OnboardingCreateAccountView: View {
    @Environment(HighlighterStore.self) private var store

    @State private var displayName: String = ""
    @State private var username: String = ""
    @State private var usernameState: UsernameState = .idle
    @State private var isWorking = false
    @State private var errorMessage: String?
    @State private var createdAccount: GeneratedAccount?
    @State private var navigateToInterests = false

    @FocusState private var focusedField: Field?

    private enum Field { case displayName, username }

    private var checkTask: Task<Void, Never>? = nil

    var body: some View {
        ZStack {
            Color.highlighterPaper.ignoresSafeArea()

            VStack(alignment: .leading, spacing: 0) {
                Spacer()

                VStack(alignment: .leading, spacing: 8) {
                    Text("What should we call you?")
                        .font(.system(.title, design: .default).weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)

                    Text("Display name is visible to everyone. Username lets others find you on Nostr.")
                        .font(.callout)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineSpacing(2)
                }
                .padding(.horizontal, 32)
                .padding(.bottom, 32)

                VStack(spacing: 12) {
                    TextField("Display name", text: $displayName)
                        .font(.title3)
                        .textInputAutocapitalization(.words)
                        .autocorrectionDisabled()
                        .padding(.horizontal, 20)
                        .padding(.vertical, 16)
                        .background(.thinMaterial, in: .rect(cornerRadius: 16))
                        .padding(.horizontal, 32)
                        .focused($focusedField, equals: .displayName)
                        .onSubmit { focusedField = .username }
                        .onChange(of: displayName) { _, new in
                            if username.isEmpty {
                                let suggested = slugify(new)
                                if !suggested.isEmpty {
                                    username = suggested
                                    scheduleCheck(for: suggested)
                                }
                            }
                        }

                    usernameField
                }

                if let msg = errorMessage {
                    Text(msg)
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .padding(.horizontal, 32)
                        .padding(.top, 8)
                }

                Spacer()

                VStack(spacing: 12) {
                    Button(action: createAccount) {
                        Group {
                            if isWorking {
                                ProgressView().tint(.white)
                            } else {
                                Text("Continue")
                                    .font(.headline)
                            }
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                    }
                    .buttonStyle(.glassProminent)
                    .disabled(isWorking || displayName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !usernameAllowsContinue)
                    .padding(.horizontal, 32)

                    NavigationLink {
                        LoginView()
                    } label: {
                        Text("I already have an account")
                            .font(.footnote)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
                .padding(.bottom, 48)
            }
        }
        .navigationDestination(isPresented: $navigateToInterests) {
            if let account = createdAccount {
                OnboardingInterestsView(account: account)
            }
        }
        .onAppear { focusedField = .displayName }
    }

    private var usernameField: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 0) {
                TextField("username", text: $username)
                    .font(.title3)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.asciiCapable)
                    .focused($focusedField, equals: .username)
                    .onChange(of: username) { _, new in
                        let normalized = new.lowercased()
                        if normalized != new { username = normalized }
                        scheduleCheck(for: normalized)
                    }
                    .onSubmit { createAccount() }

                usernameTrailingIndicator
                    .frame(width: 28)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 16)
            .background(.thinMaterial, in: .rect(cornerRadius: 16))
            .padding(.horizontal, 32)

            usernameCaption
                .padding(.horizontal, 36)
                .animation(.easeInOut(duration: 0.15), value: usernameState)
        }
    }

    @ViewBuilder
    private var usernameTrailingIndicator: some View {
        switch usernameState {
        case .checking:
            ProgressView().scaleEffect(0.7)
        case .available:
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
        case .taken:
            Image(systemName: "xmark.circle.fill")
                .foregroundStyle(.red)
        case .invalid:
            Image(systemName: "exclamationmark.circle.fill")
                .foregroundStyle(.orange)
        case .idle:
            EmptyView()
        }
    }

    @ViewBuilder
    private var usernameCaption: some View {
        switch usernameState {
        case .available(let identifier, _):
            Text("\(identifier)")
                .font(.caption)
                .foregroundStyle(.green)
        case .taken:
            Text("Already taken")
                .font(.caption)
                .foregroundStyle(.red)
        case .invalid:
            Text("Only letters, numbers, - and _")
                .font(.caption)
                .foregroundStyle(.orange)
        default:
            EmptyView()
        }
    }

    private var usernameAllowsContinue: Bool {
        if username.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return true }
        if case .available = usernameState { return true }
        return false
    }

    private func scheduleCheck(for name: String) {
        usernameState = .idle
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        guard isValidUsername(trimmed) else {
            usernameState = .invalid
            return
        }

        usernameState = .checking

        Task {
            try? await Task.sleep(nanoseconds: 400_000_000)
            guard username == trimmed else { return }
            await checkAvailability(name: trimmed)
        }
    }

    private func checkAvailability(name: String) async {
        var components = URLComponents(url: nip05BaseURL.appendingPathComponent("api/nip05"), resolvingAgainstBaseURL: false)!
        components.queryItems = [URLQueryItem(name: "name", value: name)]
        guard let url = components.url else { return }

        do {
            let (data, _) = try await URLSession.shared.data(from: url)
            let decoded = try JSONDecoder().decode(Nip05AvailabilityResponse.self, from: data)
            guard username == name else { return }
            if decoded.available {
                let domain = decoded.identifier.components(separatedBy: "@").last ?? "highlighter.com"
                usernameState = .available(identifier: decoded.identifier, domain: domain)
            } else {
                usernameState = .taken
            }
        } catch {
            guard username == name else { return }
            usernameState = .idle
        }
    }

    private func createAccount() {
        let name = displayName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, !isWorking else { return }
        guard usernameAllowsContinue else { return }

        isWorking = true
        errorMessage = nil

        Task {
            defer { isWorking = false }
            do {
                let account = try await store.safeCore.generateAccount()
                AppSessionStore.shared.persistNsec(account.nsec)

                let claimedUsername: String
                if case .available(let identifier, let domain) = usernameState, !username.isEmpty {
                    let eventJson = try await store.safeCore.signNip05RegistrationAuth(
                        name: username,
                        domain: domain
                    )
                    let authEvent = try JSONDecoder().decode(RawNostrEvent.self, from: Data(eventJson.utf8))
                    try await registerNip05(name: username, auth: authEvent)
                    claimedUsername = identifier
                } else {
                    claimedUsername = ""
                }

                Task {
                    try? await store.safeCore.updateProfile(
                        name: "",
                        displayName: name,
                        about: "",
                        picture: "",
                        banner: "",
                        nip05: claimedUsername,
                        website: "",
                        lud16: ""
                    )
                }

                createdAccount = account
                navigateToInterests = true
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    private func registerNip05(name: String, auth: RawNostrEvent) async throws {
        let url = nip05BaseURL.appendingPathComponent("api/nip05")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        let body = Nip05RegisterRequest(name: name, auth: auth)
        request.httpBody = try JSONEncoder().encode(body)

        let (data, response) = try await URLSession.shared.data(for: request)
        let statusCode = (response as? HTTPURLResponse)?.statusCode ?? 0
        if statusCode < 200 || statusCode >= 300 {
            let msg = (try? JSONDecoder().decode(ErrorResponse.self, from: data))?.error ?? "Registration failed (\(statusCode))"
            throw RegistrationError.server(msg)
        }
    }

    private func isValidUsername(_ value: String) -> Bool {
        let re = try! NSRegularExpression(pattern: "^[a-z0-9_-]{1,64}$")
        let range = NSRange(value.startIndex..., in: value)
        return re.firstMatch(in: value, range: range) != nil
    }

    private func slugify(_ text: String) -> String {
        text
            .lowercased()
            .components(separatedBy: .whitespacesAndNewlines)
            .joined(separator: "_")
            .filter { $0.isLetter || $0.isNumber || $0 == "_" || $0 == "-" }
    }
}

// MARK: - Supporting types

private struct Nip05AvailabilityResponse: Decodable {
    let available: Bool
    let identifier: String
}

private struct RawNostrEvent: Codable {
    let id: String
    let pubkey: String
    let created_at: Int
    let kind: Int
    let tags: [[String]]
    let content: String
    let sig: String
}

private struct Nip05RegisterRequest: Encodable {
    let name: String
    let auth: RawNostrEvent
}

private struct ErrorResponse: Decodable {
    let error: String
}

private enum RegistrationError: LocalizedError {
    case server(String)
    var errorDescription: String? {
        if case .server(let msg) = self { return msg }
        return nil
    }
}
