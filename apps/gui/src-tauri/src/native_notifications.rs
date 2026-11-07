use std::{collections::HashMap, sync::Mutex};
use tauri::Emitter;
use tauri::Manager;
use tauri_plugin_notification::{NotificationExt, PermissionState};

#[derive(Clone)]
struct Receipt {
    server: String,
    scope: String,
    instance: String,
    id: i64,
}
#[derive(Default)]
struct DeliveryState {
    shown: HashMap<i32, Receipt>,
    reads: HashMap<String, i64>,
}
#[derive(Default)]
struct Notifications(Mutex<DeliveryState>);
fn receipt_key(server: &str, scope: &str, instance: &str) -> String {
    serde_json::json!([server, scope, instance]).to_string()
}

#[tauri::command]
pub fn system_notification_permission(
    app: tauri::AppHandle,
    request: bool,
) -> Result<&'static str, String> {
    let notification = app.notification();
    let status = if request {
        notification.request_permission()
    } else {
        notification.permission_state()
    }
    .map_err(|e| e.to_string())?;
    Ok(match status {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        _ => "default",
    })
}

#[tauri::command]
pub fn show_system_notification(
    app: tauri::AppHandle,
    id: i32,
    title: String,
    body: String,
    path: String,
    server: String,
    scope: Option<String>,
    instance: Option<String>,
    event_id: Option<i64>,
) -> Result<(), String> {
    if !path.starts_with("/chat/") && !path.starts_with("/slack?channel=") {
        return Err("Invalid notification target".into());
    }
    let record = Receipt {
        server: server.clone(),
        scope: scope.unwrap_or_default(),
        instance: instance.unwrap_or_default(),
        id: event_id.unwrap_or(i64::from(id)),
    };
    let state = app.state::<Notifications>();
    let mut state = state
        .0
        .lock()
        .map_err(|_| "Notification state unavailable")?;
    let key = receipt_key(&record.server, &record.scope, &record.instance);
    if state
        .reads
        .get(&key)
        .is_some_and(|cursor| *cursor >= record.id)
    {
        return Ok(());
    }
    app.notification()
        .builder()
        .id(id)
        .title(title)
        .body(body)
        .extra("path", path)
        .extra("server", server)
        .extra("scope", record.scope.clone())
        .extra("instance", record.instance.clone())
        .extra("notification_id", record.id)
        .show()
        .map_err(|e| e.to_string())?;
    state.shown.insert(id, record);
    Ok(())
}

#[tauri::command]
pub fn dismiss_system_notifications(
    app: tauri::AppHandle,
    server: String,
    scope: String,
    instance: String,
    cursor: i64,
) -> Result<(), String> {
    let state = app.state::<Notifications>();
    let mut state = state
        .0
        .lock()
        .map_err(|_| "Notification state unavailable")?;
    let read = state
        .reads
        .entry(receipt_key(&server, &scope, &instance))
        .or_insert(cursor);
    *read = (*read).max(cursor);
    let through = *read;
    let mut ids: Vec<i32> = state
        .shown
        .iter()
        .filter_map(|(id, record)| {
            (record.server == server
                && record.scope == scope
                && record.instance == instance
                && record.id <= through)
                .then_some(*id)
        })
        .collect();
    #[cfg(mobile)]
    for item in app.notification().active().map_err(|e| e.to_string())? {
        let extra = item.extra();
        if extra.get("server").and_then(|v| v.as_str()) == Some(server.as_str())
            && extra.get("scope").and_then(|v| v.as_str()) == Some(scope.as_str())
            && extra.get("instance").and_then(|v| v.as_str()) == Some(instance.as_str())
            && extra
                .get("notification_id")
                .and_then(|v| v.as_i64())
                .is_some_and(|id| id <= through)
        {
            ids.push(item.id());
        }
    }
    ids.sort_unstable();
    ids.dedup();
    app.notification()
        .remove_active(ids.clone())
        .map_err(|e| e.to_string())?;
    for id in ids {
        state.shown.remove(&id);
    }
    Ok(())
}

pub fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    app.manage(Notifications::default());
    let handle = app.handle().clone();
    app.notification().on_action(move |action| {
        if action.action_id() != "tap" {
            return;
        }
        if let Some(notification) = action.notification() {
            #[cfg(desktop)]
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            let _ = handle.emit("omnistream-notification", notification.extra());
        }
    })?;
    Ok(())
}
