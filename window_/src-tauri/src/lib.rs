mod capture;
mod commands;
mod encoder;
mod mic;
mod mixer;
mod process_list;
mod tray;

use tauri::Manager;

pub use commands::RecorderState;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,audio_recorder_lib=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let state = RecorderState::new(app.handle().clone());
            app.manage(state);
            tray::setup(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_recordable_processes,
            commands::start_recording,
            commands::stop_recording,
            commands::get_recording_state,
            commands::check_permissions,
            commands::open_system_settings,
            commands::default_recordings_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
