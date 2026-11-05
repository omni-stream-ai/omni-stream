#[cfg(desktop)]
mod desktop {
    use serde::{Deserialize, Serialize};
    use tauri::Manager;
    use tauri_plugin_autostart::ManagerExt;

    pub struct CoreChild(pub std::sync::Mutex<Option<tauri_plugin_shell::process::CommandChild>>);

    #[derive(Default, Deserialize, Serialize)]
    #[serde(default, rename_all = "camelCase")]
    pub struct Preferences {
        pub allow_lan: bool,
    }

    fn preferences_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
        app.path()
            .app_config_dir()
            .map(|p| p.join("desktop.json"))
            .map_err(|e| e.to_string())
    }

    pub fn load(app: &tauri::AppHandle) -> Result<Preferences, String> {
        match std::fs::read(preferences_path(app)?) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Settings {
        allow_lan: bool,
        autostart: bool,
    }

    #[tauri::command]
    pub fn desktop_settings(app: tauri::AppHandle) -> Result<Settings, String> {
        Ok(Settings {
            allow_lan: load(&app)?.allow_lan,
            autostart: app.autolaunch().is_enabled().map_err(|e| e.to_string())?,
        })
    }

    #[tauri::command]
    pub fn set_desktop_lan(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
        let path = preferences_path(&app)?;
        std::fs::create_dir_all(path.parent().ok_or("Missing settings directory")?)
            .map_err(|e| e.to_string())?;
        let temporary = path.with_extension("tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec(&Preferences { allow_lan: enabled }).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temporary, path).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn set_desktop_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
        let autolaunch = app.autolaunch();
        if enabled {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        }
        .map_err(|e| e.to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_shell::init());
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            desktop::desktop_settings,
            desktop::set_desktop_lan,
            desktop::set_desktop_autostart
        ]);
    builder
        .setup(|app| {
            #[cfg(desktop)]
            {
                use tauri_plugin_shell::ShellExt;
                let preferences = desktop::load(app.handle()).map_err(std::io::Error::other)?;
                let address = if preferences.allow_lan {
                    "0.0.0.0:4317"
                } else {
                    "127.0.0.1:4317"
                };
                let command = app.shell().sidecar("omnistream")?.args(["serve", address]);
                let (mut events, child) = command.spawn()?;
                tauri::Manager::manage(app, desktop::CoreChild(std::sync::Mutex::new(Some(child))));
                tauri::async_runtime::spawn(async move { while events.recv().await.is_some() {} });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building OmniStream GUI")
        .run(|app, event| {
            #[cfg(desktop)]
            if matches!(event, tauri::RunEvent::Exit) {
                use tauri::Manager;
                if let Some(child) = app.state::<desktop::CoreChild>().0.lock().unwrap().take() {
                    let _ = child.kill();
                }
            }
        });
}
