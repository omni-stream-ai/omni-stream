mod native_notifications;
mod remote_push;

#[cfg(desktop)]
mod desktop {
    use serde::{Deserialize, Serialize};
    use tauri::Manager;
    use tauri_plugin_autostart::ManagerExt;

    pub struct CoreChild(pub std::sync::Mutex<Option<tauri_plugin_shell::process::CommandChild>>);
    pub struct LocalToken(pub String);

    pub fn new_token() -> Result<String, String> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| "Unable to generate local Core credential")?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    #[tauri::command]
    pub fn desktop_client_token(token: tauri::State<'_, LocalToken>) -> String {
        token.0.clone()
    }

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
    let builder = tauri::Builder::default()
        .append_invoke_initialization_script(format!(
            "Object.defineProperty(window, '__OMNISTREAM_MOBILE__', {{ value: {} }}); Object.defineProperty(window, '__OMNISTREAM_ANDROID__', {{ value: {} }});",
            cfg!(mobile), cfg!(target_os = "android")
        ))
        .plugin(tauri_plugin_notification::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(remote_push::init());
    #[cfg(mobile)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        native_notifications::system_notification_permission,
        native_notifications::dismiss_system_notifications,
        remote_push::remote_push,
        native_notifications::show_system_notification
    ]);
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            desktop::desktop_client_token,
            desktop::desktop_settings,
            desktop::set_desktop_lan,
            desktop::set_desktop_autostart,
            native_notifications::system_notification_permission,
            native_notifications::dismiss_system_notifications,
            remote_push::remote_push,
            native_notifications::show_system_notification
        ]);
    builder
        .setup(|app| {
            native_notifications::setup(app)?;
            #[cfg(desktop)]
            {
                use tauri_plugin_shell::ShellExt;
                let preferences = desktop::load(app.handle()).map_err(std::io::Error::other)?;
                let address = if preferences.allow_lan {
                    "0.0.0.0:4317"
                } else {
                    "127.0.0.1:4317"
                };
                let token = desktop::new_token().map_err(std::io::Error::other)?;
                let command = app
                    .shell()
                    .sidecar("omnistream")?
                    .args(["serve", address])
                    .env("OMNISTREAM_DESKTOP_TOKEN", &token);
                let (mut events, child) = command.spawn()?;
                tauri::Manager::manage(app, desktop::CoreChild(std::sync::Mutex::new(Some(child))));
                tauri::Manager::manage(app, desktop::LocalToken(token));
                tauri::async_runtime::spawn(async move { while events.recv().await.is_some() {} });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building OmniStream GUI")
        .run(|_app, _event| {
            #[cfg(desktop)]
            if matches!(_event, tauri::RunEvent::Exit) {
                use tauri::Manager;
                if let Some(child) = _app.state::<desktop::CoreChild>().0.lock().unwrap().take() {
                    let _ = child.kill();
                }
            }
        });
}
