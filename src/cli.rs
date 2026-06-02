use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "deepseek-cursor-proxy-rust",
    version,
    about = "Local DeepSeek compatibility proxy rebuilt in Rust"
)]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,

    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 9000)]
    pub port: u16,

    #[arg(long)]
    pub verbose: bool,

    #[arg(long = "no-display-reasoning", default_value_t = false)]
    pub no_display_reasoning: bool,

    #[arg(long = "no-collapsible-reasoning", default_value_t = false)]
    pub no_collapsible_reasoning: bool,

    #[arg(long)]
    pub trace_dir: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub clear_reasoning_cache: bool,

    #[arg(long, default_value_t = false)]
    pub tunnel: bool,

    #[arg(long, default_value = "cloudflared")]
    pub tunnel_provider: String,

    #[arg(long)]
    pub cloudflared_bin: Option<PathBuf>,

    #[arg(long = "no-sync-cursor-base-url", default_value_t = false)]
    pub no_sync_cursor_base_url: bool,

    #[arg(long)]
    pub cursor_state_db: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn parses_core_flags() {
        let cli = Cli::parse_from([
            "deepseek-cursor-proxy-rust",
            "--host",
            "0.0.0.0",
            "--port",
            "9100",
            "--verbose",
            "--trace-dir",
            "/tmp/traces",
            "--clear-reasoning-cache",
            "--tunnel",
            "--tunnel-provider",
            "cloudflared",
            "--cloudflared-bin",
            "/opt/homebrew/bin/cloudflared",
            "--cursor-state-db",
            "/tmp/state.vscdb",
        ]);

        assert_eq!(cli.host, "0.0.0.0");
        assert_eq!(cli.port, 9100);
        assert!(cli.verbose);
        assert_eq!(cli.trace_dir, Some(PathBuf::from("/tmp/traces")));
        assert!(cli.clear_reasoning_cache);
        assert!(cli.tunnel);
        assert_eq!(cli.tunnel_provider, "cloudflared");
        assert_eq!(
            cli.cloudflared_bin,
            Some(PathBuf::from("/opt/homebrew/bin/cloudflared"))
        );
        assert_eq!(cli.cursor_state_db, Some(PathBuf::from("/tmp/state.vscdb")));
        assert!(!cli.no_sync_cursor_base_url);
    }
}
