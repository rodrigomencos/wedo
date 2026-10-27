# todo

A small terminal checklist that you and your agents share. You keep it open in a
TUI; agents read and edit the same list through the CLI or an MCP server, and their
changes show up in the TUI as they happen.

## Install

```sh
cargo install --path .
```

This puts `todo` in `~/.cargo/bin`.

## The TUI

Run `todo` with no arguments.

| Key | Action |
| --- | --- |
| `j` / `k`, `↓` / `↑` | Move the selection |
| `g` / `G` | Jump to the first / last item |
| `space`, `enter` | Check or uncheck the item |
| `a` | Add an item |
| `e` | Edit the item |
| `d` | Delete the item (asks first) |
| `J` / `K` | Move the item down / up |
| `c` | Delete all finished items (asks first) |
| `q`, `esc` | Quit |

While typing an item: `enter` saves, `esc` cancels, `ctrl-a` / `ctrl-e` jump to the
start / end, `ctrl-u` clears back to the start.

### Colors

The TUI has no color scheme of its own; it uses your terminal's theme. Text uses
the theme's palette (blue as the accent, green for checked items), and secondary
text is dimmed. The selection bar is a tint of your terminal's background,
read from the terminal at startup when it supports truecolor and color queries;
otherwise the selected row is shown in reverse video. `NO_COLOR` is respected.

## The CLI

```sh
todo ls                         # everything; --pending or --done to filter
todo add "buy milk" "call mom"  # one item per argument
todo done 1 2                   # check off by id
todo undo 2                     # uncheck
todo edit 2 "call mom back"
todo rm 1
todo clear                      # delete all finished items
```

Add `--json` to any command for machine-readable output. Commands that take
several ids are all-or-nothing: if one id doesn't exist, nothing changes and the
command exits non-zero.

## Giving agents access

### MCP

`todo mcp` runs an MCP server over stdio with these tools: `list_items`,
`add_items`, `set_done`, `edit_item`, `remove_items` and `clear_done`.

Claude Code:

```sh
claude mcp add --scope user todo -- todo mcp
```

Other clients (Claude Desktop, etc.) take the usual config. Use the absolute path,
since GUI apps don't see your shell's `PATH`:

```json
{
  "mcpServers": {
    "todo": { "command": "/Users/you/.cargo/bin/todo", "args": ["mcp"] }
  }
}
```

### CLI

Any agent with a shell can use the CLI directly. Something like this in a
`CLAUDE.md` / `AGENTS.md` is enough:

```md
My checklist is managed with the `todo` command. Use `todo --json ls` to read it,
`todo add "..."` to add items and `todo done <id>` to check them off. Ids are stable.
```

## Where the list lives

`$XDG_DATA_HOME/todo/todo.json`, or `~/.local/share/todo/todo.json` when that isn't
set. Point at another list with `--file path` or `TODO_FILE=path`; a separate file
per project works fine.

It's plain JSON, so you can edit it by hand. Every write takes a lock on
`todo.json.lock`, re-reads the file, applies the change and atomically renames a
temp file into place. The TUI, the CLI and any number of agents can write at once
without losing each other's changes.
