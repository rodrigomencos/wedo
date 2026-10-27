# wedo

[![CI](https://github.com/rodrigomencos/wedo/actions/workflows/ci.yml/badge.svg)](https://github.com/rodrigomencos/wedo/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**A terminal checklist you share with your AI agents.** You keep it open in a TUI.
Claude Code, Codex, Cursor or any other agent reads and updates the same list, through
MCP or the command line, and you watch their changes land live, highlighted, with who
made them.

```text
┌ lists ────────────┐┌ auth-refactor ───────────────────────────────────── 2/6 ┐
│› auth-refactor 2/6││  1 backend ───────────────────────────────────────── 2/4│
│  default       0/2││  2 [✓] Add a sessions table                             │
│  groceries     1/3││  3 [✓] Hash passwords with argon2                       │
│                   ││› 4 [ ] Write the login handler                          │
│                   ││  5 [ ] Rate-limit failed logins                         │
│                   ││  6 tests ─────────────────────────────────────────── 0/2│
│                   ││  7 [ ] Cover expired tokens                             │
│                   ││  8 [ ] Cover the lockout after five failed attempts in a│
│                   ││        row, including the reset after an hour           │
│                   ││                                                         │
└──────────────── L ┘└──────────────── ~/.local/share/wedo/auth-refactor.json ┘
space check  a add  e edit  d delete  / search  u undo  ? all keys  q quit
```

- **Live.** Changes from agents appear within a quarter of a second and stay
  highlighted for a few seconds, with a note like `claude-code: checked 2 · added 1`.
- **A list per job.** Named lists sit in a sidebar. Ask an agent to keep its own list
  for a session and follow along.
- **Sections.** Items written as `# Title` become headings with their own progress.
- **Built for long lists.** Hide finished items, search, jump to the next open item,
  undo, copy an item's text, use the mouse.
- **Safe with many writers.** Every write locks the list, rereads it and replaces it
  atomically, so you and any number of agents never overwrite each other.
- **Plain files.** Each list is a small JSON file you can read, edit or commit.
- **Your colors.** No theme of its own: it uses your terminal's palette and respects
  `NO_COLOR`.

## Install

With Rust 1.89 or newer:

```sh
cargo install --git https://github.com/rodrigomencos/wedo
```

Or from a clone, `cargo install --path .`. Either puts `wedo` in `~/.cargo/bin`.

## Quick start

```sh
wedo add "write the README" "tag a release"   # add to your default list
wedo                                          # open the TUI
```

Give Claude Code access (other agents are [below](#connecting-agents)):

```sh
claude mcp add --scope user wedo -- wedo mcp
```

Then, in a new Claude Code session:

> Keep a wedo list called auth-refactor for this session and check items off as you go.

The list appears in the sidebar of any open TUI, or open it with `wedo -l auth-refactor`.

## The TUI

`wedo` opens your default list and `wedo -l <name>` a named one. Press `?` inside for
every key.

| Key | Action |
| --- | --- |
| `j` `k` / `↓` `↑` | Move |
| `g` `G` | First / last item |
| `n` `N` | Next / previous unfinished item |
| `space` / `enter` | Check or uncheck |
| `a` | Add at the end |
| `o` `O` | Add below / above the selection |
| `e` | Edit |
| `d` | Delete (asks first) |
| `J` `K` | Move the item down / up |
| `c` | Delete all finished items (asks first) |
| `u` | Undo |
| `h` | Hide or show finished items |
| `/` | Search; `esc` clears it |
| `y` | Copy the item's text |
| `tab` `shift-tab` | Next / previous list |
| `←` / `L` | Go to the lists sidebar. There, `j` `k` switch lists as you move, `→` or `enter` come back, `n` creates a list and `d` deletes one |
| `m` | Turn the mouse off (to select text) or back on |
| `q` / `esc` | Quit |

While typing, `enter` saves, `esc` cancels, `ctrl-a` / `ctrl-e` jump to the start or
end, and `ctrl-u` clears back to the start.

**Mouse.** Click an item to select it, click its checkbox to check it, scroll to move,
and click a list in the sidebar to open it.

**Sections.** An item written as `# Backend` is a heading. It shows how many items in
its section are done, can't be checked off itself, and doesn't count toward totals.

**Changes from elsewhere.** When an agent, or another terminal, changes the list, the
items it touched light up for a few seconds and the footer says who did what.

## The CLI

```sh
wedo ls                          # the list; --pending or --done to filter
wedo add "oat milk" "coffee"     # one item per argument
wedo add --after 2 "basil"       # insert after item 2 (0 for the top)
wedo add "# Pantry"              # a section heading
wedo done 1 2                    # check off
wedo undo 2                      # uncheck
wedo edit 2 "decaf coffee"
wedo rm 1
wedo clear                       # delete all finished items
wedo lists                       # every list and its progress
```

Every command takes `-l <name>` to work on a named list, and `--json` for
machine-readable output. Commands given several numbers are all-or-nothing: if one
doesn't exist, nothing changes and the command exits non-zero.

| Variable | Effect |
| --- | --- |
| `WEDO_LIST` | The list to use when `-l` isn't given |
| `WEDO_FILE` | Use this file instead of a named list, like `--file` |
| `WEDO_AUTHOR` | The name the TUI shows for changes made through the CLI (default `cli`) |
| `XDG_DATA_HOME` | Where lists live; see the [FAQ](#faq) |
| `NO_COLOR` | Turn colors off |

Flags beat environment variables.

## Connecting agents

### MCP

`wedo mcp` runs an MCP server over stdio with these tools:

| Tool | What it does |
| --- | --- |
| `list_items` | Read a list, optionally only `pending` or `done` items |
| `add_items` | Add items at the end, or `after` a given item |
| `set_done` | Check items off, or uncheck them |
| `edit_item` | Change an item's text |
| `remove_items` | Delete items |
| `clear_done` | Delete all finished items |
| `list_checklists` | Every list and its progress |

Each item tool takes an optional `list`, so one server covers every list, and a new
name creates that list. Changes are credited to the client's name, such as
`claude-code`.

**Claude Code**

```sh
claude mcp add --scope user wedo -- wedo mcp
```

**Codex CLI**, in `~/.codex/config.toml`:

```toml
[mcp_servers.wedo]
command = "wedo"
args = ["mcp"]
```

**Cursor** (`~/.cursor/mcp.json`), **Gemini CLI** (`~/.gemini/settings.json`) and
**Claude Desktop** (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "wedo": { "command": "wedo", "args": ["mcp"] }
  }
}
```

**VS Code**, in `.vscode/mcp.json`:

```json
{
  "servers": {
    "wedo": { "type": "stdio", "command": "wedo", "args": ["mcp"] }
  }
}
```

Desktop apps often don't see your shell's `PATH`. If the server doesn't start, use the
full path that `which wedo` prints.

### Any agent with a shell

The CLI works just as well. A note like this in `CLAUDE.md` or `AGENTS.md` is enough:

```md
Track multi-step work with the `wedo` CLI: `wedo -l <task> add "..."` to plan,
`wedo -l <task> --json ls` to read, and `wedo -l <task> done <n>` to check off.
Numbers are positions, so re-read the list before acting on old ones.
```

### A checklist per session

Ask the agent to keep a list of its own, for example: *"Keep a wedo list called
auth-refactor for this session and check items off as you go."* Through MCP it passes
`list: "auth-refactor"`; through the CLI it runs `wedo -l auth-refactor ...`. The list
shows up in the sidebar of any open TUI.

## FAQ

**Where are my lists?** Each list is `<name>.json` in `$XDG_DATA_HOME/wedo/`, which is
usually `~/.local/share/wedo/`, or `%APPDATA%\wedo\` on Windows. Names can use letters,
digits, `-`, `_` and `.`. To keep a list anywhere else, such as inside a project, pass
`--file path`.

**Why did an item's number change?** Numbers are positions: 1 is the top, and
deleting, clearing or moving items renumbers the rest, so there are never gaps. Agents
are told this and re-read the list before acting on old numbers.

**What if several agents write at once?** Each write takes a lock
(`<name>.json.lock`), rereads the list, applies its change and atomically renames a new
file into place. Nobody overwrites anyone else, and readers never see a half-written
file.

**Can I edit a list by hand?** Yes, it's plain JSON:

```json
{
  "next_id": 3,
  "items": [
    { "id": 1, "text": "# Backend", "done": false },
    { "id": 2, "text": "Add a sessions table", "done": true }
  ],
  "changed_by": "claude-code"
}
```

Only each item's `text` and `done` matter; numbers are recomputed whenever the file is
read.

**How does it pick colors?** It has no theme of its own. Text uses colors from your
terminal's palette (blue for accents, green for checked items), and the selection bar
is a tint of your terminal's background, which it asks the terminal for at startup.
Terminals that can't answer get reverse video instead. `NO_COLOR` turns colors off
while keeping bold, dim and strikethrough.

**I can't select text with the mouse.** The TUI captures the mouse for clicks and
scrolling. Most terminals let you hold shift (option on some macOS terminals) to select
anyway, or press `m` to turn the mouse off.

**Does it run on Windows?** Yes. CI runs the tests on Linux, macOS and Windows.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

`docs/demo.tape` records a demo GIF with [vhs](https://github.com/charmbracelet/vhs):
`vhs docs/demo.tape`.

## License

Licensed under either the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT license](LICENSE-MIT), at your option. Contributions are accepted under the same
terms.
