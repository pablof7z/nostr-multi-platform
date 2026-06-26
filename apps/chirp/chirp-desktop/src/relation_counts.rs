use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RelationCounts {
    #[serde(default)]
    pub replies: RelationCount,
    #[serde(default)]
    pub reactions: RelationCount,
    #[serde(default)]
    pub reposts: RelationCount,
    #[serde(default)]
    pub zaps: RelationCount,
}

impl RelationCounts {
    pub(crate) fn summary(&self) -> String {
        format!(
            "reply {}  react {}  repost {}  zap {}",
            self.replies.label(),
            self.reactions.label(),
            self.reposts.label(),
            self.zaps.label()
        )
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RelationCount {
    Known {
        #[serde(default)]
        count: u64,
    },
    Loading {
        #[serde(default, rename = "interest")]
        _interest: Option<serde_json::Value>,
    },
}

impl Default for RelationCount {
    fn default() -> Self {
        Self::Loading { _interest: None }
    }
}

impl RelationCount {
    fn label(&self) -> String {
        match self {
            Self::Known { count } => count.to_string(),
            Self::Loading { .. } => "...".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RelationCounts;

    #[test]
    fn summary_defaults_to_loading_labels() {
        let counts = RelationCounts::default();

        assert_eq!(
            counts.summary(),
            "reply ...  react ...  repost ...  zap ..."
        );
    }

    #[test]
    fn decodes_known_and_loading_counts() {
        let json = serde_json::json!({
            "replies": { "state": "known", "count": 2 },
            "reactions": { "state": "known", "count": 3 },
            "reposts": { "state": "known", "count": 1 },
            "zaps": {
                "state": "loading",
                "interest": { "namespace": "nmp.nip01.visible_note_relations" }
            }
        });

        let counts: RelationCounts =
            serde_json::from_value(json).expect("relation counts deserialize");

        assert_eq!(counts.summary(), "reply 2  react 3  repost 1  zap ...");
    }
}
