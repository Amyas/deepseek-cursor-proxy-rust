mod state;

use crate::cli::Cli;
use crate::config::AppConfig;
use crate::cursor::state::update_cursor_openai_base_url;
use crate::error::{AppError, AppResult};
use crate::http::routes::build_router;
use crate::tunnel::cloudflared::CloudflaredQuickTunnel;
pub use state::AppState;

pub struct Application {
    state: AppState,
    clear_only: bool,
}

impl Application {
    pub async fn bootstrap(cli: Cli) -> AppResult<Self> {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(if cli.verbose { "info" } else { "warn" })
            .try_init();
        let config_path = AppConfig::config_path_or_default(cli.config.clone());
        let mut config = AppConfig::load_or_create(&config_path)
            .map_err(|error| AppError::Config(error.to_string()))?;
        config.host = cli.host;
        config.port = cli.port;
        config.verbose = cli.verbose;
        config.display_reasoning = !cli.no_display_reasoning;
        config.collapsible_reasoning = !cli.no_collapsible_reasoning;
        config.trace_dir = cli.trace_dir;
        config.tunnel_enabled = cli.tunnel;
        config.tunnel_provider = cli.tunnel_provider;
        config.cloudflared_bin = cli.cloudflared_bin;
        config.sync_cursor_openai_base_url = !cli.no_sync_cursor_base_url;
        if let Some(cursor_state_db) = cli.cursor_state_db {
            config.cursor_state_db_path = cursor_state_db;
        }
        let clear_only = cli.clear_reasoning_cache;

        Ok(Self {
            state: AppState::new(config)?,
            clear_only,
        })
    }

    pub async fn run(self) -> AppResult<()> {
        if self.clear_only {
            let cleared =
                crate::reasoning::store::ReasoningStore::clear(self.state.store.as_ref())?;
            tracing::info!(cleared, "cleared reasoning cache");
            return Ok(());
        }
        let bind_address = self.state.config.bind_address();
        let listener = tokio::net::TcpListener::bind(&bind_address)
            .await
            .map_err(|error| AppError::Config(error.to_string()))?;
        tracing::info!(
            bind = %bind_address,
            "listening"
        );
        let local_url = format!("http://{}", bind_address);
        let tunnel = if self.state.config.tunnel_enabled {
            match self.state.config.tunnel_provider.as_str() {
                "cloudflared" => {
                    let tunnel = CloudflaredQuickTunnel::start(
                        &local_url,
                        self.state.config.cloudflared_bin.as_deref(),
                    )?;
                    if self.state.config.sync_cursor_openai_base_url {
                        match update_cursor_openai_base_url(
                            &self.state.config.cursor_state_db_path,
                            &tunnel.public_url,
                        ) {
                            Ok(update) => {
                                tracing::info!(
                                    cursor_state_db = %update.db_path.display(),
                                    old_url = ?update.old_url,
                                    new_url = %update.new_url,
                                    backup_path = %update.backup_path.display(),
                                    "updated Cursor Override OpenAI Base URL"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    cursor_state_db = %self.state.config.cursor_state_db_path.display(),
                                    %error,
                                    "failed to update Cursor Override OpenAI Base URL"
                                );
                            }
                        }
                    }
                    tracing::info!(
                        public_base_url = %format!("{}/v1", tunnel.public_url),
                        "quick tunnel ready"
                    );
                    Some(tunnel)
                }
                other => {
                    return Err(AppError::Config(format!(
                        "unsupported tunnel provider: {other}"
                    )));
                }
            }
        } else {
            None
        };

        let server = axum::serve(listener, build_router(self.state).into_make_service())
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            });
        let result = server
            .await
            .map_err(|error| AppError::Upstream(error.to_string()));
        drop(tunnel);
        result
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::Application;
    use crate::cli::Cli;

    #[tokio::test]
    async fn bootstrap_applies_cli_overrides() {
        let cli = Cli {
            config: None,
            host: "127.0.0.1".to_string(),
            port: 9100,
            verbose: true,
            no_display_reasoning: true,
            no_collapsible_reasoning: false,
            trace_dir: None,
            clear_reasoning_cache: false,
            tunnel: true,
            tunnel_provider: "cloudflared".to_string(),
            cloudflared_bin: None,
            no_sync_cursor_base_url: false,
            cursor_state_db: None,
        };

        let app = Application::bootstrap(cli).await.unwrap();
        assert_eq!(app.state().config.port, 9100);
        assert!(app.state().config.verbose);
        assert!(!app.state().config.display_reasoning);
        assert!(app.state().config.collapsible_reasoning);
        assert!(app.state().config.tunnel_enabled);
        assert_eq!(app.state().config.tunnel_provider, "cloudflared");
        assert!(app.state().config.sync_cursor_openai_base_url);
    }
}
