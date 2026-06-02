use clap::Parser;
use deepseek_cursor_proxy_rust::app::Application;
use deepseek_cursor_proxy_rust::cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let exit_code = match Application::bootstrap(cli).await {
        Ok(app) => match app.run().await {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{error}");
                1
            }
        },
        Err(error) => {
            eprintln!("{error}");
            1
        }
    };

    std::process::exit(exit_code);
}
