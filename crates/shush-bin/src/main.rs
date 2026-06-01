mod cli;
mod fe_master;
mod remote_tmux;
mod server;
mod session_manager;
mod tmux_control;

use clap::Parser;
use cli::{Cli, Command, ServerCmd};
use server::create_app;
use session_manager::SessionManager;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Server(cmd) => start_server(cmd).await,
        Command::Client(_) => {
            eprintln!("client commands not yet implemented");
            std::process::exit(1);
        }
    }
}

async fn start_server(cmd: ServerCmd) {
    let session_manager = SessionManager::new();
    let app = create_app(session_manager);
    let bind_addr = format!("127.0.0.1:{}", cmd.port);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|_| panic!("failed to bind to {bind_addr}"));

    tracing::info!("listening on http://{bind_addr}");

    axum::serve(listener, app).await.expect("server error");
}
