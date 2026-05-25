/// Wire type for a Nostr user profile, decoded from the Rust-owned profile
/// projection before it reaches the Ratatui view layer.
///
/// `npub` and `npub_short` are already Rust-formatted. TUI code should render
/// them as provided rather than reformatting keys locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileWire {
    pub pubkey: String,
    pub display_name: Option<String>,
    pub about: Option<String>,
    pub picture_url: Option<String>,
    pub nip05: Option<String>,
    pub npub: String,
    pub npub_short: String,
}

impl ProfileWire {
    pub fn display(&self) -> &str {
        non_empty(self.display_name.as_deref()).unwrap_or(self.npub_short.as_str())
    }

    pub fn nip05(&self) -> Option<&str> {
        non_empty(self.nip05.as_deref())
    }

    pub fn picture_url(&self) -> Option<&str> {
        non_empty(self.picture_url.as_deref())
    }

    pub fn initials(&self) -> String {
        let label = self.display();
        let mut letters = label
            .split_whitespace()
            .filter_map(|part| part.chars().next())
            .filter(|ch| ch.is_alphanumeric())
            .take(2)
            .collect::<String>();
        if letters.is_empty() {
            letters = self.npub_short.chars().take(2).collect();
        }
        letters.to_uppercase()
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
