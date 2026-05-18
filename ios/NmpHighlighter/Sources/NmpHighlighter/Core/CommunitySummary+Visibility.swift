extension CommunitySummary {
    var isPublicOpenRoom: Bool {
        visibility == "public" && access == "open"
    }
}
