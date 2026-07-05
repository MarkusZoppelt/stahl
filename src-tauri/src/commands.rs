//! Tauri command handlers invoked by the GUI frontend.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{env, fs, sync::Arc};

use tauri::State;
use tokio::sync::RwLock;

use portfolio_rs::{
    agent_skill::SKILL_CONTENT,
    config::{AppConfig, ThemeMode},
    config_path,
    context::PortfolioContext,
    doctor::WorkspaceHealth,
    review::Review,
    simulate::RebalanceSimulation,
    state::{
        AllocationItemDto, AppConfigDto, AppState, CreatePositionRequest, GeneratedDocument,
        PerformanceDto, PolicyDto, PortfolioSummaryDto, PositionDto, UpdatePositionRequest,
    },
    validate::ValidationReport,
};

pub struct ConfigState {
    pub config: RwLock<AppConfig>,
}

impl ConfigState {
    pub fn load() -> Result<Self, String> {
        AppConfig::load()
            .map(|config| Self {
                config: RwLock::new(config),
            })
            .map_err(|e| format!("{:#}", e))
    }

    pub async fn save(&self) -> Result<(), String> {
        let cfg = self.config.read().await;
        cfg.save().map_err(|e| format!("{:#}", e))?;
        drop(cfg);
        restrict_config_file_permissions();
        Ok(())
    }
}

/// The config file may contain an LLM API key; lock it down to owner-only
/// read/write (the same convention as `~/.aws/credentials`) instead of the
/// default, umask-dependent (often world-readable) permissions that `confy`
/// creates the file with.
#[cfg(unix)]
fn restrict_config_file_permissions() {
    let Ok(path) = config_path() else {
        return;
    };
    if let Err(e) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
        eprintln!("Warning: failed to restrict config file permissions: {e:#}");
    }
}

#[cfg(not(unix))]
fn restrict_config_file_permissions() {}

#[tauri::command]
pub async fn load_portfolio(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<PortfolioSummaryDto, String> {
    state
        .load_file(&path)
        .await
        .map_err(|e| format!("{:#}", e))?;
    Ok(state.get_portfolio_summary().await)
}

#[tauri::command]
pub async fn get_portfolio_summary(
    state: State<'_, Arc<AppState>>,
) -> Result<PortfolioSummaryDto, String> {
    Ok(state.get_portfolio_summary().await)
}

#[tauri::command]
pub async fn get_positions(state: State<'_, Arc<AppState>>) -> Result<Vec<PositionDto>, String> {
    Ok(state.get_positions().await)
}

#[tauri::command]
pub async fn get_allocation(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<AllocationItemDto>, String> {
    Ok(state.get_allocation().await)
}

#[tauri::command]
pub async fn get_performance(state: State<'_, Arc<AppState>>) -> Result<PerformanceDto, String> {
    Ok(state.get_performance().await)
}

#[tauri::command]
pub async fn refresh_prices(
    state: State<'_, Arc<AppState>>,
) -> Result<PortfolioSummaryDto, String> {
    state.refresh_prices().await;
    Ok(state.get_portfolio_summary().await)
}

#[tauri::command]
pub async fn add_position(
    state: State<'_, Arc<AppState>>,
    request: CreatePositionRequest,
) -> Result<PositionDto, String> {
    state
        .add_position(request)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn update_position(
    state: State<'_, Arc<AppState>>,
    id: usize,
    request: UpdatePositionRequest,
) -> Result<PositionDto, String> {
    state
        .update_position(id, request)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn delete_position(state: State<'_, Arc<AppState>>, id: usize) -> Result<(), String> {
    state
        .delete_position(id)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn save_portfolio(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.save().await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn get_file_path(state: State<'_, Arc<AppState>>) -> Result<Option<String>, String> {
    Ok(state.file_path().await)
}

#[tauri::command]
pub async fn set_currency(
    state: State<'_, Arc<AppState>>,
    cfg_state: State<'_, Arc<ConfigState>>,
    currency: String,
) -> Result<(), String> {
    state.set_currency(currency.clone()).await;
    {
        let mut cfg = cfg_state.config.write().await;
        cfg.set_currency(currency);
    }
    cfg_state.save().await
}

#[tauri::command]
pub fn get_initial_file() -> Option<String> {
    let mut args = env::args().skip(1);
    args.find(|arg| !arg.starts_with('-'))
}

// --- Workspace & onboarding ---

#[tauri::command]
pub async fn load_workspace(
    state: State<'_, Arc<AppState>>,
    cfg_state: State<'_, Arc<ConfigState>>,
    dir: String,
) -> Result<PortfolioSummaryDto, String> {
    {
        let mut cfg = cfg_state.config.write().await;
        state
            .load_workspace(&dir, &mut cfg)
            .await
            .map_err(|e| format!("{:#}", e))?;
    }
    cfg_state.save().await?;
    Ok(state.get_portfolio_summary().await)
}

#[tauri::command]
pub async fn load_simple_file(
    state: State<'_, Arc<AppState>>,
    cfg_state: State<'_, Arc<ConfigState>>,
    path: String,
) -> Result<PortfolioSummaryDto, String> {
    {
        let mut cfg = cfg_state.config.write().await;
        state
            .load_simple_file(&path, &mut cfg)
            .await
            .map_err(|e| format!("{:#}", e))?;
    }
    cfg_state.save().await?;
    Ok(state.get_portfolio_summary().await)
}

#[tauri::command]
pub async fn init_workspace(dir: String, dry_run: bool) -> Result<(), String> {
    portfolio_rs::workspace::init_workspace(&dir, dry_run).map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn get_workspace_dir(state: State<'_, Arc<AppState>>) -> Result<Option<String>, String> {
    Ok(state.workspace_dir().await)
}

#[tauri::command]
pub async fn init_agent_files(dir: String, dry_run: bool) -> Result<(), String> {
    portfolio_rs::workspace::init_agent_files(&dir, dry_run).map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn export_skill(dir: String, dry_run: bool) -> Result<(), String> {
    portfolio_rs::agent_skill::skill_export(&dir, dry_run).map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub fn get_skill_content() -> String {
    SKILL_CONTENT.to_string()
}

// --- Policy ---

#[tauri::command]
pub async fn load_policy(
    state: State<'_, Arc<AppState>>,
    path: Option<String>,
) -> Result<PolicyDto, String> {
    state
        .load_policy(path)
        .await
        .map_err(|e| format!("{:#}", e))?;
    state
        .policy()
        .await
        .map(PolicyDto::from)
        .ok_or_else(|| "policy not loaded".to_string())
}

#[tauri::command]
pub async fn get_policy(state: State<'_, Arc<AppState>>) -> Result<Option<PolicyDto>, String> {
    Ok(state.policy().await.map(PolicyDto::from))
}

#[tauri::command]
pub async fn set_policy_from_toml(
    state: State<'_, Arc<AppState>>,
    toml: String,
) -> Result<PolicyDto, String> {
    state
        .set_policy_from_toml(&toml)
        .await
        .map_err(|e| format!("{:#}", e))?;
    state
        .policy()
        .await
        .map(PolicyDto::from)
        .ok_or_else(|| "policy not set".to_string())
}

#[tauri::command]
pub async fn save_policy(
    state: State<'_, Arc<AppState>>,
    toml: String,
) -> Result<PolicyDto, String> {
    state
        .set_policy_from_toml(&toml)
        .await
        .map_err(|e| format!("{:#}", e))?;
    let policy = state
        .policy()
        .await
        .ok_or_else(|| "policy not set".to_string())?;
    let path = state
        .policy_path()
        .await
        .ok_or_else(|| "no workspace loaded to save policy into".to_string())?;
    fs::write(&path, toml).map_err(|e| format!("failed to write policy file: {:#}", e))?;
    Ok(PolicyDto::from(policy))
}

#[tauri::command]
pub async fn generate_policy_from_strategy(strategy: String) -> Result<PolicyDto, String> {
    portfolio_rs::policy::policy_from_strategy(&strategy)
        .map(PolicyDto::from)
        .ok_or_else(|| format!("unknown strategy: {}", strategy))
}

// --- Analysis ---

#[tauri::command]
pub async fn get_context(state: State<'_, Arc<AppState>>) -> Result<PortfolioContext, String> {
    state.get_context().await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn get_review(state: State<'_, Arc<AppState>>) -> Result<Review, String> {
    state.get_review().await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn get_simulation(
    state: State<'_, Arc<AppState>>,
) -> Result<RebalanceSimulation, String> {
    state.get_simulation().await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn run_doctor(
    state: State<'_, Arc<AppState>>,
    dir: Option<String>,
) -> Result<WorkspaceHealth, String> {
    state.run_doctor(dir).await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn validate_portfolio(
    state: State<'_, Arc<AppState>>,
) -> Result<ValidationReport, String> {
    state
        .validate_portfolio()
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn generate_report(
    state: State<'_, Arc<AppState>>,
    dir: Option<String>,
    dry_run: bool,
) -> Result<GeneratedDocument, String> {
    state
        .generate_report(dir, dry_run)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn draft_decision(
    state: State<'_, Arc<AppState>>,
    title: Option<String>,
    dir: Option<String>,
    dry_run: bool,
) -> Result<GeneratedDocument, String> {
    state
        .draft_decision(title, dir, dry_run)
        .await
        .map_err(|e| format!("{:#}", e))
}

// --- Settings / config ---

#[tauri::command]
pub async fn get_config(cfg_state: State<'_, Arc<ConfigState>>) -> Result<AppConfigDto, String> {
    let cfg = cfg_state.config.read().await;
    Ok(AppConfigDto::from(&*cfg))
}

#[tauri::command]
pub async fn set_theme(
    cfg_state: State<'_, Arc<ConfigState>>,
    theme: ThemeMode,
) -> Result<(), String> {
    {
        let mut cfg = cfg_state.config.write().await;
        cfg.set_theme(theme);
    }
    cfg_state.save().await
}

#[tauri::command]
pub async fn set_llm_config(
    cfg_state: State<'_, Arc<ConfigState>>,
    provider_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
) -> Result<(), String> {
    {
        let mut cfg = cfg_state.config.write().await;
        if let Some(url) = provider_url {
            cfg.set_llm_provider_url(url);
        }
        if let Some(key) = api_key {
            cfg.set_llm_api_key(key);
        }
        if let Some(model) = model {
            cfg.set_llm_model(model);
        }
    }
    cfg_state.save().await
}

#[tauri::command]
pub async fn clear_remembered_path(cfg_state: State<'_, Arc<ConfigState>>) -> Result<(), String> {
    {
        let mut cfg = cfg_state.config.write().await;
        cfg.clear_paths();
    }
    cfg_state.save().await
}

#[tauri::command]
pub async fn get_config_path() -> Result<String, String> {
    config_path().map_err(|e| format!("{:#}", e))
}
