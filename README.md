# wedo

A small terminal checklist that you and your agents share. You keep it open in a
TUI; agents read and edit the same list through the CLI or an MCP server, and their
changes show up in the TUI as they happen.

## Install

```sh
cargo install --path .
```

This puts `wedo` in `~/.cargo/bin`.

## The TUI

Run `wedo` with no arguments for your default list, or `wedo -l <name>` for a named
one.

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
| `tab` / `shift-tab` | Switch to the next / previous list |
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
wedo ls                         # everything; --pending or --done to filter
wedo add "buy milk" "call mom"  # one item per argument
wedo done 1 2                   # check off by id
wedo undo 2                     # uncheck
wedo edit 2 "call mom back"
wedo rm 1
wedo clear                      # delete all finished items
wedo lists                      # every list, with how many items are done
```

Every command works on the `default` list unless you pick another with
`-l <name>` / `--list <name>`, or `WEDO_LIST=<name>`. A list is created the first
time something is written to it:

```sh
wedo -l auth-refactor add "write migration" "update tests"
wedo -l auth-refactor done 1
```

Add `--json` to any command for machine-readable output. Commands that take
several ids are all-or-nothing: if one id doesn't exist, nothing changes and the
command exits non-zero.

## Giving agents access

### MCP

`wedo mcp` runs an MCP server over stdio with these tools: `list_items`,
`add_items`, `set_done`, `edit_item`, `remove_items`, `clear_done` and
`list_checklists`. Each item tool takes an optional `list` name, so one server
covers every list.

Claude Code:

```sh
claude mcp add --scope user wedo -- wedo mcp
```

Other clients (Claude Desktop, etc.) take the usual config. Use the absolute path,
since GUI apps don't see your shell's `PATH`:

```json
{
  "mcpServers": {
    "wedo": { "command": "/Users/you/.cargo/bin/wedo", "args": ["mcp"] }
  }
}
```

### CLI

Any agent with a shell can use the CLI directly. Something like this in a
`CLAUDE.md` / `AGENTS.md` is enough:

```md
My checklists are managed with the `wedo` command. Use `wedo --json ls` to read one,
`wedo add "..."` to add items and `wedo done <id>` to check them off. Ids are stable.
Pick a list with `-l <name>`; `wedo lists` shows them all.
```

### A checklist per session

Ask the agent to track its work in a list of its own, for example: *"Keep a todo
list called auth-refactor for this session and check items off as you go."* Through
MCP it passes `list: "auth-refactor"`; through the CLI it runs
`wedo -l auth-refactor ...`. Watch it with `wedo -l auth-refactor`, or `tab` to it from
any open TUI.

## Where lists live

Each list is `<name>.json` in `$XDG_DATA_HOME/wedo/`, or `~/.local/share/wedo/` when
that isn't set. Names can use letters, digits, `-`, `_` and `.`. To use a file
anywhere else, pass `--file path` or set `WEDO_FILE=path` instead of a list name.
Flags beat environment variables.

Lists are plain JSON, so you can edit them by hand. Every write takes a lock on
`<name>.json.lock`, re-reads the file, applies the change and atomically renames a
temp file into place. The TUI, the CLI and any number of agents can write at once
without losing each other's changes.
