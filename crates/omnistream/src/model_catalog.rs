use crate::{config::Config, storage::Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Model {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProviderModels {
    pub models: Vec<Model>,
    pub reasoning: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Selection {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub reasoning: Option<String>,
}

pub fn selection(store: &Store, config: &Config) -> Result<Selection, String> {
    match store.setting("model-selection")? {
        Some(value) => serde_json::from_value(value).map_err(|e| e.to_string()),
        None => Ok(Selection {
            provider: config.model_provider.clone(),
            model: config.model.clone(),
            reasoning: config.model_reasoning_effort.clone(),
        }),
    }
}

pub fn apply(config: &Config) -> Result<Config, String> {
    let selected = selection(&Store::open(&crate::state_path())?, config)?;
    let mut result = config.clone();
    result.model_provider = selected.provider;
    result.model = selected.model;
    result.model_reasoning_effort = selected.reasoning.filter(|level| !level.is_empty());
    Ok(result)
}

fn parse_models(
    value: &Value,
    config: &Config,
    provider_id: &str,
) -> Result<ProviderModels, String> {
    let entries = value
        .as_array()
        .or_else(|| value["data"].as_array())
        .or_else(|| value["models"].as_array())
        .ok_or("模型接口未返回模型列表")?;
    config
        .model_providers
        .get(provider_id)
        .ok_or("provider 未配置")?;
    let mut models = std::collections::BTreeMap::new();
    let api_reasoning: Option<Vec<String>> = value
        .get("reasoning_efforts")
        .or_else(|| value.get("reasoning_levels"))
        .map(|levels| parse_reasoning(levels))
        .transpose()?;
    for entry in entries {
        let Some(id) = entry["id"]
            .as_str()
            .or_else(|| entry["slug"].as_str())
            .or_else(|| entry.as_str())
            .filter(|id| !id.trim().is_empty())
        else {
            continue;
        };
        models.insert(
            id.to_string(),
            Model {
                id: id.into(),
                name: entry["display_name"]
                    .as_str()
                    .or_else(|| entry["name"].as_str())
                    .unwrap_or(id)
                    .into(),
            },
        );
    }
    Ok(ProviderModels {
        models: models.into_values().collect(),
        reasoning: api_reasoning.unwrap_or_else(|| config.reasoning_efforts(provider_id)),
    })
}

fn parse_reasoning(levels: &Value) -> Result<Vec<String>, String> {
    let levels = levels.as_array().ok_or("思考等级字段必须是数组")?;
    let mut result = Vec::new();
    for level in levels {
        if let Some(level) = level
            .as_str()
            .or_else(|| level["reasoning_effort"].as_str())
            .or_else(|| level["effort"].as_str())
            .or_else(|| level["id"].as_str())
        {
            if !level.is_empty() && !result.iter().any(|existing| existing == level) {
                result.push(level.to_string());
            }
        }
    }
    Ok(result)
}

pub fn list(
    store: &Store,
    config: &Config,
    provider_id: &str,
    refresh: bool,
) -> Result<ProviderModels, String> {
    let provider = config
        .model_providers
        .get(provider_id)
        .ok_or("provider 未配置")?;
    let base = provider
        .base_url
        .as_deref()
        .ok_or("provider 未配置 base_url")?;
    let identity = format!(
        "{}:{}:{:?}:{:?}:{:?}",
        provider_id,
        base,
        provider.models,
        provider.reasoning_efforts,
        config.model_reasoning_efforts
    );
    let key = format!("model-catalog:{:x}", Sha256::digest(identity.as_bytes()));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    if !refresh {
        if let Some(cached) = store.setting(&key)? {
            if now.saturating_sub(cached["at"].as_u64().unwrap_or(0)) < 300 {
                return serde_json::from_value(cached["models"].clone()).map_err(|e| e.to_string());
            }
        }
    }
    let mut provider_config = config.clone();
    provider_config.model_provider = Some(provider_id.into());
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut request = client.get(format!("{}/models", base.trim_end_matches('/')));
    if let Some(key) = provider_config.api_key()? {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .map_err(|e| format!("无法获取模型列表：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("模型列表接口返回 HTTP {}", response.status()));
    }
    let value: Value = response
        .json()
        .map_err(|e| format!("模型列表格式无效：{e}"))?;
    let models = parse_models(&value, config, provider_id)?;
    store.save_setting(&key, &json!({"at":now,"models":models}))?;
    Ok(models)
}

pub fn save_selection(
    store: &Store,
    config: &Config,
    selected: Selection,
) -> Result<Selection, String> {
    let provider = selected.provider.as_deref().ok_or("请选择 provider")?;
    let models = list(store, config, provider, false)?;
    models
        .models
        .iter()
        .find(|model| Some(&model.id) == selected.model.as_ref())
        .ok_or("模型不在接口返回的列表中")?;
    if let Some(level) = selected.reasoning.as_deref().filter(|s| !s.is_empty()) {
        if !models.reasoning.iter().any(|item| item == level) {
            return Err("该 provider 不支持此思考等级".into());
        }
    }
    store.save_setting(
        "model-selection",
        &serde_json::to_value(&selected).map_err(|e| e.to_string())?,
    )?;
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_reasoning_metadata_overrides_config() {
        let config: Config = toml::from_str("[model_providers.test]\nmodels=[{id='a',reasoning=['low']},{id='b',reasoning=['high']}]\n").unwrap();
        let models = parse_models(&json!({"data":[{"id":"a","reasoning":["max"]},{"id":"b","reasoning_efforts":[]}],"reasoning_efforts":["max"]}), &config, "test").unwrap();
        assert_eq!(models.reasoning, ["max"]);
        assert_eq!(models.models.len(), 2);
    }
    #[test]
    fn selection_is_global_and_survives_store_reopen() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.db");
        let store = Store::open(&path).unwrap();
        let chosen = Selection {
            provider: Some("test".into()),
            model: Some("api-model".into()),
            reasoning: Some("max".into()),
        };
        store
            .save_setting("model-selection", &serde_json::to_value(&chosen).unwrap())
            .unwrap();
        store
            .save_conversation_settings("a", &Default::default())
            .unwrap();
        store
            .save_conversation_settings("b", &Default::default())
            .unwrap();
        drop(store);
        assert_eq!(
            selection(&Store::open(&path).unwrap(), &Config::default()).unwrap(),
            chosen
        );
    }
}
