use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const MIN_COLUMNS: u32 = 1;
pub const MAX_COLUMNS: u32 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub last_folder: Option<String>,
    pub include_subfolders: bool,
    pub column_count: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_folder: None,
            include_subfolders: false,
            column_count: 5,
        }
    }
}

fn clamp_settings(mut settings: Settings) -> Settings {
    settings.column_count = settings.column_count.clamp(MIN_COLUMNS, MAX_COLUMNS);
    settings.last_folder = settings
        .last_folder
        .filter(|folder| !folder.trim().is_empty());
    settings
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|err| err.to_string())?;
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir.join("settings.json"))
}

#[tauri::command]
pub fn load_settings(app: AppHandle) -> Settings {
    let Ok(path) = settings_path(&app) else {
        return Settings::default();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Settings::default();
    };
    serde_json::from_str(&text)
        .map(clamp_settings)
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let settings = clamp_settings(settings);
    let path = settings_path(&app)?;
    let text = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    fs::write(path, text).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_column_count() {
        let high = clamp_settings(Settings {
            column_count: 40,
            ..Settings::default()
        });
        assert_eq!(high.column_count, MAX_COLUMNS);
        let low = clamp_settings(Settings {
            column_count: 0,
            last_folder: Some("  ".into()),
            ..Settings::default()
        });
        assert_eq!(low.column_count, MIN_COLUMNS);
        assert_eq!(low.last_folder, None);
    }
}
