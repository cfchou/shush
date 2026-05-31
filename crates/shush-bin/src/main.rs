mod cli;
mod fe_master;
mod remote_tmux;
mod server;
mod session_manager;
mod tmux_control;

use clap::Parser;
use cli::{Cli, Command};
use server::create_app;
use session_manager::SessionManager;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Server(_) => start_server().await,
        Command::Client(_) => {
            eprintln!("client commands not yet implemented");
            std::process::exit(1);
        }
    }
}

async fn start_server() {
    let session_manager = SessionManager::new();
    let app = create_app(session_manager);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8100")
        .await
        .expect("failed to bind to 127.0.0.1:8100");

    tracing::info!("listening on http://127.0.0.1:8100");

    axum::serve(listener, app).await.expect("server error");
}
