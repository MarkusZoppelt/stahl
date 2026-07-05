//! Tauri desktop GUI entry point for portfolio_rs.

#[cfg(target_os = "linux")]
use std::env;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use portfolio_rs::{config::AppMode, state::AppState};
use tauri::{Manager, async_runtime, generate_context};

mod commands;

use commands::ConfigState;

fn main() {
    // WebKit2GTK's dmabuf renderer hits a Wayland protocol error
    // ("Error 71 (Protocol error) dispatching to Wayland display") on many
    // compositors. Disable it so WebKit falls back to the older, stable
    // rendering path. Harmless on X11. This must be set here (rather than
    // only in packaging, e.g. a Nix wrapper) so it also applies to plain
    // `cargo build`/`cargo tauri dev`/`cargo tauri build` runs, not just
    // binaries built through the flake.
    #[cfg(target_os = "linux")]
    if env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: this runs as the very first statement in `main`, before
        // any other threads (tokio runtime, webview) are spawned, so there
        // is no concurrent access to the environment.
        unsafe {
            env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    let config_state = Arc::new(ConfigState::load().unwrap_or_else(|e| {
        eprintln!("Warning: failed to load config: {:#}", e);
        ConfigState {
            config: tokio::sync::RwLock::new(portfolio_rs::AppConfig::default()),
        }
    }));

    let initial_currency = config_state.config.blocking_read().currency.clone();

    let state = Arc::new(AppState::new(initial_currency));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state.clone())
        .manage(config_state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::load_portfolio,
            commands::get_portfolio_summary,
            commands::get_positions,
            commands::get_allocation,
            commands::get_performance,
            commands::refresh_prices,
            commands::add_position,
            commands::update_position,
            commands::delete_position,
            commands::save_portfolio,
            commands::get_file_path,
            commands::set_currency,
            commands::get_initial_file,
            commands::load_workspace,
            commands::load_simple_file,
            commands::init_workspace,
            commands::get_workspace_dir,
            commands::init_agent_files,
            commands::export_skill,
            commands::get_skill_content,
            commands::load_policy,
            commands::get_policy,
            commands::set_policy_from_toml,
            commands::save_policy,
            commands::generate_policy_from_strategy,
            commands::get_context,
            commands::get_review,
            commands::get_simulation,
            commands::run_doctor,
            commands::validate_portfolio,
            commands::generate_report,
            commands::draft_decision,
            commands::get_config,
            commands::set_theme,
            commands::set_llm_config,
            commands::clear_remembered_path,
            commands::get_config_path,
        ])
        .setup(move |app| {
            let state = state.clone();
            let config_state = config_state.clone();

            async_runtime::spawn(async move {
                let cfg = config_state.config.read().await.clone();
                let _ = auto_load(&state, &cfg).await;
            });

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }
            Ok(())
        })
        .run(generate_context!())
        .expect("error while running tauri application");
}

async fn auto_load(state: &AppState, cfg: &portfolio_rs::AppConfig) -> Result<(), String> {
    if cfg.last_mode == AppMode::Workspace {
        if let Some(dir) = &cfg.workspace_dir {
            let positions_path = PathBuf::from(dir).join("positions.json");
            if positions_path.exists() {
                if let Err(e) = state.load_file(&positions_path.to_string_lossy()).await {
                    eprintln!("Warning: failed to auto-load workspace portfolio: {:#}", e);
                } else {
                    *state.workspace_dir.write().await = Some(dir.clone());
                    if let Err(e) = state.load_policy(None).await {
                        eprintln!("Warning: failed to auto-load workspace policy: {:#}", e);
                    }
                }
            }
        }
    } else if let Some(file) = cfg.effective_portfolio_file() {
        if Path::new(&file).exists() {
            if let Err(e) = state.load_file(&file).await {
                eprintln!("Warning: failed to auto-load portfolio: {:#}", e);
            }
        }
    }
    Ok(())
}
