use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "shush")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Server(ServerCmd),
    Client(ClientCmd),
}

#[derive(Debug, clap::Args)]
pub struct ServerCmd {
    #[arg(long, default_value_t = 8100)]
    pub port: u16,
}

#[derive(Debug, clap::Args)]
pub struct ClientCmd {
    #[command(subcommand)]
    pub subcommand: ClientSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ClientSubcommand {
    Submit,
    Sessions,
    Stream,
    Yolo,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_server_subcommand() {
        let cli = Cli::try_parse_from(["shush", "server"]).unwrap();
        let Command::Server(cmd) = cli.command else {
            unreachable!()
        };
        assert_eq!(cmd.port, 8100);
    }

    #[test]
    fn parse_server_subcommand_with_port_override() {
        let cli = Cli::try_parse_from(["shush", "server", "--port", "18080"]).unwrap();
        let Command::Server(cmd) = cli.command else {
            unreachable!()
        };
        assert_eq!(cmd.port, 18080);
    }

    #[test]
    fn parse_client_submit() {
        let cli = Cli::try_parse_from(["shush", "client", "submit"]).unwrap();
        assert!(matches!(cli.command, Command::Client(_)));
        let Command::Client(cmd) = cli.command else {
            unreachable!()
        };
        assert!(matches!(cmd.subcommand, ClientSubcommand::Submit));
    }

    #[test]
    fn parse_client_sessions() {
        let cli = Cli::try_parse_from(["shush", "client", "sessions"]).unwrap();
        let Command::Client(cmd) = cli.command else {
            unreachable!()
        };
        assert!(matches!(cmd.subcommand, ClientSubcommand::Sessions));
    }

    #[test]
    fn parse_client_stream() {
        let cli = Cli::try_parse_from(["shush", "client", "stream"]).unwrap();
        let Command::Client(cmd) = cli.command else {
            unreachable!()
        };
        assert!(matches!(cmd.subcommand, ClientSubcommand::Stream));
    }

    #[test]
    fn parse_client_yolo() {
        let cli = Cli::try_parse_from(["shush", "client", "yolo"]).unwrap();
        let Command::Client(cmd) = cli.command else {
            unreachable!()
        };
        assert!(matches!(cmd.subcommand, ClientSubcommand::Yolo));
    }

    #[test]
    fn reject_no_subcommand() {
        let err = Cli::try_parse_from(["shush"]).unwrap_err();
        assert!(
            err.to_string().contains("subcommand"),
            "expected subcommand error"
        );
    }
}
