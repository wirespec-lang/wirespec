use std::env;

use tower_lsp::{LspService, Server};
use wirespec_lsp::backend::Backend;

#[tokio::main]
async fn main() {
    if let Some(arg) = env::args().nth(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                eprintln!("wirespec-lsp {}", env!("CARGO_PKG_VERSION"));
                eprintln!("Language Server Protocol server for wirespec");
                eprintln!();
                eprintln!("Usage: wirespec-lsp");
                eprintln!("  Communicates via JSON-RPC over stdin/stdout.");
                return;
            }
            "--version" | "-V" => {
                println!("wirespec-lsp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("error: unknown option: {other}");
                eprintln!("Usage: wirespec-lsp [--help | --version]");
                std::process::exit(1);
            }
        }
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
