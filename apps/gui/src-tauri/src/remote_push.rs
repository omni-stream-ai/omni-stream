//! Android FCM bridge; iOS remains on local notifications until APNs is configured.
#[cfg(target_os = "android")]
pub struct RemotePush(tauri::plugin::PluginHandle<tauri::Wry>);

#[cfg(target_os = "android")]
pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri::{Emitter, Manager};
    tauri::plugin::Builder::new("remote-push")
        .setup(|app, api| {
            let handle =
                api.register_android_plugin("com.omnistreamai.app.push", "RemotePushPlugin")?;
            let events = app.clone();
            let channel = tauri::ipc::Channel::new(move |body| {
                if let tauri::ipc::InvokeResponseBody::Json(payload) = body {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) {
                        let _ = events.emit("omnistream-notification", value);
                    }
                }
                Ok(())
            });
            #[derive(serde::Serialize)]
            struct Listener {
                event: &'static str,
                handler: tauri::ipc::Channel,
            }
            handle.run_mobile_plugin::<()>(
                "registerListener",
                Listener {
                    event: "click",
                    handler: channel,
                },
            )?;
            app.manage(RemotePush(handle));
            Ok(())
        })
        .build()
}

#[tauri::command]
pub async fn remote_push(
    app: tauri::AppHandle,
    action: String,
    input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        if !["token", "saveBinding", "removeBinding", "consumeClick", "markRead"].contains(&action.as_str()) {
            return Err("Unsupported push command".into());
        }
        app.state::<RemotePush>()
            .0
            .run_mobile_plugin(&action, input)
            .map_err(|_| "Android push operation failed".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, action, input);
        Err("Remote push is currently available on Android and browsers".into())
    }
}
