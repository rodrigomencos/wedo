mod mcp;
mod store;
mod theme;
mod tui;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use store::{DEFAULT_LIST, Item, Lists, Status, Store};
use unicode_width::UnicodeWidthStr;

/// A checklist for you and your agents. Run without a command to open the TUI.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Named list to use, kept in $XDG_DATA_HOME/wedo or ~/.local/share/wedo
    /// [env: WEDO_LIST] [default: default]
    #[arg(short, long, global = true)]
    list: Option<String>,

    /// Use this file instead of a named list [env: WEDO_FILE]
    #[arg(short, long, global = true, conflicts_with = "list")]
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
    /// Add items, one per argument: wedo add "buy milk" "call mom"
    Add {
        #[arg(required = true)]
        texts: Vec<String>,
        /// Insert after this item instead of at the end (0 for the top)
        #[arg(long, value_name = "ID")]
        after: Option<u64>,
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
    /// Show every named list and how far along it is
    Lists,
    /// Serve the checklist to agents as an MCP server over stdio
    Mcp,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let lists = Lists::new(Lists::default_dir()?);
    // Either flag beats both variables, so a stray WEDO_FILE can't block --list.
    let env = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
    let file = match (cli.file, &cli.list) {
        (Some(path), _) => Some(path),
        (None, Some(_)) => None,
        (None, None) => env("WEDO_FILE").map(PathBuf::from),
    };
    let store = match &file {
        Some(path) => Store::new(path.clone()),
        None => lists.open(
            &cli.list
                .or_else(|| env("WEDO_LIST"))
                .unwrap_or(DEFAULT_LIST.into()),
        )?,
    };
    let Some(command) = cli.command else {
        // A --file list stands alone; named lists can be switched between.
        return tui::run(store, file.is_none().then_some(lists));
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
        Command::Add { texts, after } => {
            (Some("added"), store.update(|l| l.add_all(after, &texts))?)
        }
        Command::Done { ids } => (Some("checked"), store.update(|l| l.set_done(&ids, true))?),
        Command::Undo { ids } => (
            Some("unchecked"),
            store.update(|l| l.set_done(&ids, false))?,
        ),
        Command::Edit { id, text } => (Some("edited"), vec![store.update(|l| l.edit(id, &text))?]),
        Command::Rm { ids } => (Some("removed"), store.update(|l| l.remove(&ids))?),
        Command::Clear => (Some("removed"), store.update(|l| Ok(l.clear_done()))?),
        Command::Lists => return print_lists(&lists, cli.json),
        Command::Mcp => return mcp::serve(&lists, &store),
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

fn print_lists(lists: &Lists, json: bool) -> Result<()> {
    let summaries = lists.summaries()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summaries)?);
    } else if summaries.is_empty() {
        println!("no lists yet");
    } else {
        let width = summaries.iter().map(|s| s.name.width()).max().unwrap_or(0);
        for s in &summaries {
            let pad = " ".repeat(width - s.name.width());
            println!("{}{pad}  {}/{}", s.name, s.done, s.total);
        }
    }
    Ok(())
}

fn row(item: &Item) -> String {
    if item.is_heading() {
        return format!("    {:>3}  {}", item.id, item.text);
    }
    format!(
        "[{}] {:>3}  {}",
        if item.done { 'x' } else { ' ' },
        item.id,
        item.text
    )
}
