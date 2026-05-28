#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FeatureTab {
    #[default]
    Home,
    Chats,
    Groups,
    Wallet,
    Settings,
}

impl FeatureTab {
    pub const ALL: [Self; 5] = [
        Self::Home,
        Self::Chats,
        Self::Groups,
        Self::Wallet,
        Self::Settings,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Chats => "chats",
            Self::Groups => "groups",
            Self::Wallet => "wallet",
            Self::Settings => "settings",
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Chats => "Chats",
            Self::Groups => "Groups",
            Self::Wallet => "Wallet",
            Self::Settings => "Settings",
        }
    }

    #[must_use]
    pub fn from_key(ch: char) -> Option<Self> {
        match ch {
            'h' => Some(Self::Home),
            'c' => Some(Self::Chats),
            'g' => Some(Self::Groups),
            'w' => Some(Self::Wallet),
            's' => Some(Self::Settings),
            _ => None,
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    #[must_use]
    pub fn previous(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

pub struct IosFeature {
    pub area: &'static str,
    pub feature: &'static str,
    pub tui_surface: &'static str,
}

pub const IOS_FEATURES: &[IosFeature] = &[
    IosFeature {
        area: "Onboarding",
        feature: "create account, import nsec, NIP-46 bunker/nostrconnect",
        tui_surface: "settings tab + :account commands",
    },
    IosFeature {
        area: "Home",
        feature: "timeline, compose, reply, reactions, profile/thread navigation",
        tui_surface: "home tab + compose keys",
    },
    IosFeature {
        area: "Profiles",
        feature: "kind:0 metadata, follow/unfollow, edit/publish profile",
        tui_surface: "profile pane + :profile command",
    },
    IosFeature {
        area: "Chats",
        feature: "NIP-17 inbox, conversations, send DM, DM relay list",
        tui_surface: "chats tab + :dm/:dm-relays commands",
    },
    IosFeature {
        area: "Groups",
        feature: "NIP-29 discovery/join/chat/reply/react and Marmot MLS actions",
        tui_surface: "groups tab controls",
    },
    IosFeature {
        area: "Wallet",
        feature: "NWC connect, disconnect, status/balance, pay invoice",
        tui_surface: "wallet tab + :wallet commands",
    },
    IosFeature {
        area: "Settings",
        feature: "accounts, relay editor, outbox retry/cancel, diagnostics",
        tui_surface: "settings tab + :relay/:outbox/:search commands",
    },
];
