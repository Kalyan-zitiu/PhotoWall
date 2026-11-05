mod scan;
mod settings;
mod thumbs;

use std::path::PathBuf;

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
async fn pick_folder(app: AppHandle, start: Option<String>) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut builder = app.dialog().file().set_title("Open photo folder");
        if let Some(start) = start.as_deref() {
            let path = PathBuf::from(start);
            let dir = if path.is_dir() {
                path
            } else {
                path.parent()
                    .map(|parent| parent.to_path_buf())
                    .unwrap_or_default()
            };
            if dir.is_dir() {
                builder = builder.set_directory(dir);
            }
        }
        if let Some(window) = app.get_webview_window("main") {
            builder = builder.set_parent(&window);
        }
        Ok(builder.blocking_pick_folder().map(|path| path.to_string()))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("thumb", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            thumbs::enqueue(app, request.uri().to_string(), responder);
        })
        .invoke_handler(tauri::generate_handler![
            scan::scan_folder,
            scan::normalize_drop_path,
            settings::load_settings,
            settings::save_settings,
            pick_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running PhotoWall");
}
