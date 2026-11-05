use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn save(value: &Value) -> Result<Vec<Value>, String> {
    let Some(items) = value.as_array() else {
        return Ok(vec![]);
    };
    if items.len() > 20 {
        return Err("每条消息最多 20 个附件".into());
    }
    if items
        .iter()
        .map(|item| item["data"].as_str().map(str::len).unwrap_or(0))
        .sum::<usize>()
        > 28 * 1024 * 1024
    {
        return Err("附件总大小超过 20 MB".into());
    }
    let root = crate::state_path()
        .parent()
        .ok_or("状态目录无效")?
        .join("attachments");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    items
        .iter()
        .map(|item| {
            let name = item["name"].as_str().ok_or("附件缺少名称")?;
            let data = item["data"].as_str().ok_or("附件缺少数据")?;
            let (prefix, encoded) = data.split_once(',').ok_or("附件编码无效")?;
            if !prefix.starts_with("data:")
                || !prefix.ends_with(";base64")
                || encoded.len() > 14 * 1024 * 1024
            {
                return Err("附件格式无效或超过 10 MB".into());
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| "附件编码无效")?;
            if bytes.len() > 10 * 1024 * 1024 {
                return Err("附件超过 10 MB".into());
            }
            let mime = &prefix[5..prefix.len() - 7];
            let path = root.join(format!("{:x}", Sha256::digest(&bytes)));
            std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
            Ok(json!({"name":name,"type":mime,"data":data,"path":path,"id":item["id"]}))
        })
        .collect()
}

pub fn context(items: &[Value]) -> String {
    items
        .iter()
        .map(|item| {
            format!(
                "附件 {}：{}",
                item["name"].as_str().unwrap_or_default(),
                item["path"].as_str().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
