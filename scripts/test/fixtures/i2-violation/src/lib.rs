#[tauri::command]
pub fn touch() {}

pub fn use_tauri() {
    let _ = tauri::Manager;
}
