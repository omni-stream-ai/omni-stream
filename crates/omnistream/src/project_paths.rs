use std::path::{Path, PathBuf};

fn directories(parent: &Path) -> Result<Vec<PathBuf>, String> {
    let entries =
        std::fs::read_dir(parent).map_err(|e| format!("无法读取目录 {}：{e}", parent.display()))?;
    let mut paths: Vec<_> = entries
        .take(4096)
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    paths.sort();
    Ok(paths)
}

pub fn complete(input: &str) -> Result<Vec<String>, String> {
    let expanded = if input == "~" || input.starts_with("~/") {
        let home = std::env::var("HOME").map_err(|_| "无法获取主目录")?;
        format!("{}{}", home, &input[1..])
    } else {
        input.to_owned()
    };
    let path = Path::new(&expanded);
    if !path.is_absolute() {
        return Err("请输入绝对路径或 ~/ 开头的路径".into());
    }
    let (parent, prefix) = if path.is_dir() {
        (path, "")
    } else {
        (
            path.parent().ok_or("无效路径")?,
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
        )
    };
    let matches: Vec<_> = directories(parent)?
        .into_iter()
        .filter(|item| {
            item.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(prefix) && (!name.starts_with('.') || prefix.starts_with('.'))
                })
        })
        .take(100)
        .collect();
    let mut results = matches.clone();
    // Include one level below partial matches: /da suggests /data and /data/code.
    if !prefix.is_empty() {
        for item in &matches {
            if results.len() >= 100 {
                break;
            }
            if let Ok(children) = directories(item) {
                let remaining = 100 - results.len();
                results.extend(
                    children
                        .into_iter()
                        .filter(|child| {
                            !child
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .starts_with('.')
                        })
                        .take(remaining),
                );
            }
        }
    }
    Ok(results
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completes_partial_paths_and_children_without_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("data/code")).unwrap();
        std::fs::create_dir_all(root.path().join("data/dodo")).unwrap();
        std::fs::write(root.path().join("data/file"), "").unwrap();
        let paths = complete(root.path().join("da").to_str().unwrap()).unwrap();
        for suffix in ["data", "data/code", "data/dodo"] {
            assert!(paths.contains(&root.path().join(suffix).to_string_lossy().into_owned()));
        }
        assert_eq!(paths.len(), 3);
        assert_eq!(
            complete(root.path().join("data/").to_str().unwrap())
                .unwrap()
                .len(),
            2
        );
        assert!(complete("relative").is_err());
        assert!(complete(root.path().join("missing/child").to_str().unwrap()).is_err());
    }
}
