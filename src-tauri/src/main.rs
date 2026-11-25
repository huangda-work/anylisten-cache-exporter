mod commands;

fn main() {
    let state = commands::AppState::new().expect("无法读取应用设置");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(std::sync::Arc::new(state))
        .invoke_handler(tauri::generate_handler![
            commands::get_preferences,
            commands::set_data_root,
            commands::set_export_directory,
            commands::open_output_directory,
            commands::open_saved_track,
            commands::scan_music,
            commands::preview_cached_track,
            commands::get_saved_statuses,
            commands::export_tracks,
            commands::cancel_export,
        ])
        .run(tauri::generate_context!())
        .expect("无法启动 Any Listen 缓存导出");
}
