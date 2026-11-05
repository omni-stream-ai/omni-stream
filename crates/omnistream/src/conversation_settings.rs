use crate::config::{ApprovalMode, Config};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub title: Option<String>,
    pub archived: bool,
    pub approval_mode: ApprovalMode,
    // Old records may contain model fields; only the global selection is applied.
    #[serde(skip_serializing)]
    pub provider: Option<String>,
    #[serde(skip_serializing)]
    pub model: Option<String>,
    #[serde(skip_serializing)]
    pub reasoning: Option<String>,
}

impl Settings {
    pub fn apply(&self, config: &Config) -> Result<Config, String> {
        let mut result = crate::model_catalog::apply(config)?;
        result.approval_mode = Some(self.approval_mode);
        Ok(result)
    }
}
