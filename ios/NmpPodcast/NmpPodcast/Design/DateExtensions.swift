import Foundation

// MARK: - Date display helpers

extension Date {

    /// Short date only — e.g. "May 7, 2026".
    var shortDate: String {
        formatted(date: .abbreviated, time: .omitted)
    }

    /// Short date and time — e.g. "May 7, 2026, 3:45 PM".
    var shortDateTime: String {
        formatted(date: .abbreviated, time: .shortened)
    }

    /// Relative label suitable for due-date chips and row badges.
    ///
    /// Returns "Today", "Tomorrow", or "Yesterday" for the three nearest days,
    /// and falls back to `shortDate` for anything further out. Callers prefix
    /// "Due" or "Overdue ·" themselves so the label stays composable.
    var relativeDueLabel: String {
        let cal = Calendar.current
        if cal.isDateInToday(self)     { return "Today" }
        if cal.isDateInTomorrow(self)  { return "Tomorrow" }
        if cal.isDateInYesterday(self) { return "Yesterday" }
        return shortDate
    }
}
