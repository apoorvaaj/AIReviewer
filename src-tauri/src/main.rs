// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use coach_core::config::Config;
use coach_core::net::HttpsTransport;
use coach_core::reasoning::cloud::CloudEngine;
use coach_core::validate::ValidatorRules;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

/// Find the bundled resources (knowledge base + validator rules). Installed
/// builds use the resource dir; `cargo run` falls back to the repository.
fn resource_path(app: &tauri::App, rel: &str) -> PathBuf {
    if let Ok(dir) = app.path().resource_dir() {
        let p = dir.join(rel);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(rel)
}

fn main() {
    // WebView2 keeps a profile folder (cache, cookies). We point it at a
    // per-run temporary folder, run the webview in incognito mode, and delete
    // the folder on exit. Document text never goes into web storage.
    let webview_data = std::env::temp_dir().join(format!("ib-feedback-coach-webview-{}", std::process::id()));
    let webview_data_for_setup = webview_data.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let (config, config_error) = match Config::load(Config::default_path().as_deref()) {
                Ok(c) => (c, None),
                Err(e) => (Config::default(), Some(e)),
            };
            let kb = coach_core::kb::load(&resource_path(app, "knowledge-base/physics-ia")).map_err(|e| e.to_string());
            let rules = ValidatorRules::load(&resource_path(app, "validator-rules.yaml"));
            let transport = HttpsTransport::new(config.request_timeout_secs)?;
            let engine = CloudEngine::new(config.clone(), Arc::new(transport), config.api_key());

            app.manage(commands::AppState {
                kb: kb.map(Arc::new),
                rules: rules.map(Arc::new),
                config,
                config_error,
                engine: Arc::new(engine),
                session: Mutex::new(commands::Session::default()),
            });

            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("IB Feedback Coach")
                .inner_size(1120.0, 820.0)
                .min_inner_size(820.0, 600.0)
                .incognito(true)
                .data_directory(webview_data_for_setup.clone())
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::open_document,
            commands::apply_redaction,
            commands::build_payload,
            commands::confirm_and_run,
            commands::save_report,
            commands::save_receipt,
            commands::reset_session,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start IB Feedback Coach");

    app.run(move |_app, event| {
        if let RunEvent::Exit = event {
            let _ = std::fs::remove_dir_all(&webview_data);
        }
    });
}
