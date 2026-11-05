use crate::{state_path, storage, App};
use serde_json::{json, Value};
use std::path::Path;

pub const SKILL: &str = include_str!("../../../skills/omnistream-project-management/SKILL.md");
pub fn skill_context(project: &str) -> Result<String, String> {
    let mut content = SKILL.to_string();
    let mut roots = vec![crate::state_path()
        .parent()
        .ok_or("状态目录无效")?
        .join("skills")];
    if Path::new(project).is_dir() {
        roots.push(Path::new(project).join(".omnistream/skills"));
    }
    let mut files = std::collections::BTreeSet::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path().join("SKILL.md");
            if path.is_file() {
                files.insert(path.canonicalize().map_err(|e| e.to_string())?);
            }
        }
    }
    for path in files {
        let skill = std::fs::read_to_string(&path)
            .map_err(|e| format!("无法读取 skill {}：{e}", path.display()))?;
        content.push_str(&format!(
            "\n\n用户自定义 skill（{}）：\n{}",
            path.display(),
            skill
        ));
    }
    Ok(content)
}

pub(crate) fn project_path(state: &App, value: Option<&str>) -> Result<String, String> {
    let requested = value.unwrap_or_else(|| state.project());
    if requested == "default" {
        return Ok(requested.to_string());
    }
    let store = storage::Store::open(&state_path())?;
    let matches: Vec<_> = store
        .projects()?
        .into_iter()
        .filter(|project| project.name == requested || project.path == requested)
        .collect();
    match matches.as_slice() {
        [] => {
            let path = Path::new(requested)
                .canonicalize()
                .map_err(|_| format!("找不到项目：{requested}，请提供项目路径"))?;
            if !path.is_dir() {
                return Err("项目路径不是目录".into());
            }
            Ok(path.display().to_string())
        }
        [project] => Ok(project.path.clone()),
        _ => Err(format!("项目名称不唯一：{requested}，请使用项目路径")),
    }
}

pub fn execute(state: &mut App, name: &str, args: &Value) -> Result<Value, String> {
    if let Some(result) = crate::agents::tool(name, args) {
        return result;
    }
    if crate::plugin_marketplace::is_tool(name) {
        return crate::plugin_marketplace::tool(
            name,
            args,
            state.project(),
            state.config.approval_mode(),
        );
    }
    if let Some(result) = crate::automation::tool(name, args) {
        return result;
    }
    match name {
        "register_project" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.trim().is_empty())
                .ok_or("path 必须是项目目录")?;
            let name = match args.get("name") {
                None | Some(Value::Null) => None,
                Some(Value::String(name)) => Some(name.as_str()),
                _ => return Err("name 必须是字符串".into()),
            };
            let store = storage::Store::open(&state_path())?;
            Ok(json!({"project": store.register_named_project(Path::new(path), name)?}))
        }
        "list_projects" => {
            let store = storage::Store::open(&state_path())?;
            Ok(json!({"projects": store.projects()?}))
        }
        "rename_project" | "delete_project" | "archive_project" => {
            let requested = args
                .get("project")
                .and_then(Value::as_str)
                .ok_or("project 参数不能为空")?;
            let project = project_path(state, Some(requested))?;
            let store = storage::Store::open(&state_path())?;
            match name {
                "rename_project" => {
                    let new_name = args
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or("name 参数不能为空")?;
                    Ok(json!({"project": store.rename_project(&project, new_name)?}))
                }
                "delete_project" => {
                    store.delete_project(&project)?;
                    Ok(json!({"project": project, "deleted": true}))
                }
                _ => {
                    store.delete_project(&project)?;
                    Ok(json!({"project": project, "archived": true}))
                }
            }
        }
        _ => Err(format!("未知工具：{name}")),
    }
}
