mod mcp;
mod store;
mod theme;
mod tui;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use store::{Item, Status, Store};

/// A checklist for you and your agents. Run without a command to open the TUI.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Checklist file [default: $XDG_DATA_HOME/todo/todo.json or ~/.local/share/todo/todo.json]
    #[arg(short, long, global = true, env = "TODO_FILE")]
    file: Option<PathBuf>,

    /// Print JSON instead of text
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show items
    #[command(visible_alias = "ls")]
    List {
        /// Only items not done yet
        #[arg(long, conflicts_with = "done")]
        pending: bool,
        /// Only finished items
        #[arg(long)]
        done: bool,
    },
    /// Add items, one per argument: todo add "buy milk" "call mom"
    Add {
        #[arg(required = true)]
        texts: Vec<String>,
    },
    /// Check items off
    Done {
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// Uncheck items
    Undo {
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// Replace an item's text
    Edit { id: u64, text: String },
    /// Delete items
    #[command(visible_alias = "remove")]
    Rm {
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// Delete every finished item
    Clear,
    /// Serve the checklist to agents as an MCP server over stdio
    Mcp,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::new(match cli.file {
        Some(path) => path,
        None => Store::default_path()?,
    });
    let Some(command) = cli.command else {
        return tui::run(store);
    };
    let (verb, items) = match command {
        Command::List { pending, done } => {
            let status = match (pending, done) {
                (true, _) => Status::Pending,
                (_, true) => Status::Done,
                _ => Status::All,
            };
            (None, store.load()?.filtered(status))
        }
        Command::Add { texts } => (
            Some("added"),
            store.update(|l| texts.iter().map(|t| l.add(t)).collect())?,
        ),
        Command::Done { ids } => (Some("checked"), store.update(|l| l.set_done(&ids, true))?),
        Command::Undo { ids } => (
            Some("unchecked"),
            store.update(|l| l.set_done(&ids, false))?,
        ),
        Command::Edit { id, text } => (Some("edited"), vec![store.update(|l| l.edit(id, &text))?]),
        Command::Rm { ids } => (Some("removed"), store.update(|l| l.remove(&ids))?),
        Command::Clear => (Some("removed"), store.update(|l| Ok(l.clear_done()))?),
        Command::Mcp => return mcp::serve(&store),
    };
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if items.is_empty() {
        println!("nothing here");
    } else {
        for item in &items {
            match verb {
                Some(verb) => println!("{verb} {}", row(item)),
                None => println!("{}", row(item)),
            }
        }
    }
    Ok(())
}

fn row(item: &Item) -> String {
    format!(
        "[{}] {:>3}  {}",
        if item.done { 'x' } else { ' ' },
        item.id,
        item.text
    )
}
