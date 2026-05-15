#![forbid(unsafe_code)]

mod cmds;
pub mod output;
pub mod progress;

use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "argyph",
    version = env!("CARGO_PKG_VERSION"),
    about = "Semantic code indexer for AI agents",
    long_about = "Indexes repositories into a hybrid symbol-graph + embedding store and serves queries via MCP or CLI."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Start the MCP server on stdio")]
    Serve,
    #[command(about = "Index a repository")]
    Index,
    #[command(about = "Show indexing status")]
    Status,
    #[command(about = "Search indexed code")]
    Search {
        #[arg(help = "Natural-language or structured query")]
        query: String,
    },
    #[command(about = "List extracted symbols")]
    Symbols {
        #[arg(help = "Optional path filter")]
        path: Option<String>,
    },
    #[command(about = "Export the symbol graph")]
    Graph {
        #[arg(help = "Optional path to limit the graph")]
        path: Option<String>,
    },
    #[command(about = "Pack a repository for AI consumption")]
    Pack {
        #[arg(help = "Path to the repository root")]
        path: String,
    },
    #[command(about = "Print environment diagnostics")]
    Doctor,
    #[command(about = "Install Argyph agent lookup instructions")]
    Init {
        #[arg(help = "Directory to initialize in (defaults to cwd)")]
        path: Option<String>,
    },
}

pub fn dispatch(command: Command) -> ExitCode {
    match command {
        Command::Serve => cmds::serve::run(),
        Command::Index => cmds::index::run(),
        Command::Status => cmds::status::run(),
        Command::Search { query } => cmds::search::run(&query),
        Command::Symbols { path } => cmds::symbols::run(path.as_deref()),
        Command::Graph { path } => cmds::graph::run(path.as_deref()),
        Command::Pack { path } => cmds::pack::run(&path),
        Command::Doctor => cmds::doctor::run(),
        Command::Init { path } => cmds::init::run(path.as_deref()),
    }
}
