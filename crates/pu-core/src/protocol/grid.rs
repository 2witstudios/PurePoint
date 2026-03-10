use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GridCommand {
    Split {
        #[serde(default)]
        leaf_id: Option<u32>,
        #[serde(default = "default_axis")]
        axis: String,
    },
    Close {
        #[serde(default)]
        leaf_id: Option<u32>,
    },
    Focus {
        #[serde(default)]
        leaf_id: Option<u32>,
        #[serde(default)]
        direction: Option<String>,
    },
    SetAgent {
        leaf_id: u32,
        agent_id: String,
    },
    GetLayout,
}

fn default_axis() -> String {
    "v".to_string()
}
