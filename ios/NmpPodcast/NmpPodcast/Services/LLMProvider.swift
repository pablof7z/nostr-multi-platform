import Foundation

enum LLMProvider: String, Codable, Hashable, Sendable, CaseIterable {
    case openRouter = "openrouter"
    case ollama = "ollama"

    var displayName: String {
        switch self {
        case .openRouter: return "OpenRouter"
        case .ollama:     return "Ollama Cloud"
        }
    }
}

struct LLMModelReference: Hashable, Sendable {
    let provider: LLMProvider
    let modelID: String

    init(provider: LLMProvider, modelID: String) {
        self.provider = provider
        self.modelID = modelID.trimmed
    }

    init(storedID: String) {
        let trimmed = storedID.trimmed
        if let split = trimmed.firstIndex(of: ":") {
            let prefix = String(trimmed[..<split])
            let rest = String(trimmed[trimmed.index(after: split)...])
            if let provider = LLMProvider(rawValue: prefix), !rest.isEmpty {
                self.init(provider: provider, modelID: rest)
                return
            }
        }
        self.init(provider: .openRouter, modelID: trimmed)
    }

    var storedID: String {
        switch provider {
        case .openRouter:
            return modelID
        case .ollama:
            return "\(provider.rawValue):\(modelID)"
        }
    }

    var isEmpty: Bool { modelID.isBlank }
}
