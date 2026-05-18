import OSLog

extension Logger {
    private static let subsystem = Bundle.main.bundleIdentifier ?? "com.podcast.app"

    static let audio = Logger(subsystem: subsystem, category: "Audio")
    static let player = Logger(subsystem: subsystem, category: "Player")
    static let database = Logger(subsystem: subsystem, category: "Database")
    static let network = Logger(subsystem: subsystem, category: "Network")
    static let transcription = Logger(subsystem: subsystem, category: "Transcription")
    static let ai = Logger(subsystem: subsystem, category: "AI")
    static let queue = Logger(subsystem: subsystem, category: "Queue")
    static let general = Logger(subsystem: subsystem, category: "General")
}
