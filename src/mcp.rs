//! A minimal MCP server: JSON-RPC 2.0 over newline-delimited stdio, exposing the
//! checklist as tools. Only the handful of methods a tools-only server needs.

use std::io::{self, BufRead, Write};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::store::{Lists, Status, Store};

const FALLBACK_PROTOCOL_VERSION: &str = "2025-06-18";

const INSTRUCTIONS: &str = "Checklists shared with the user, who watches them live in a terminal UI. \
Item ids are stable and never reused; call list_items to find them. \
Prefer checking items off over removing them so the user can see what was done. \
To track the work of a session or task, keep it in its own list: pass a short kebab-case \
`list` name such as \"auth-refactor\" (created on first write) and tell the user they can \
watch it with `wedo --list <name>`.";

/// `default` is the list used when a tool call doesn't name one.
pub fn serve(lists: &Lists, default: &Store) -> Result<()> {
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(lists, default, &message),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut stdout, &reply)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Answers a request. Notifications (no id) and stray responses (no method) get no reply.
fn handle(lists: &Lists, default: &Store, message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method")?.as_str()?;
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(FALLBACK_PROTOCOL_VERSION),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "wedo", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools(&default.name()) }),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call(lists, default, name, args) {
                Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                Err(e) => {
                    json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true })
                }
            }
        }
        _ => return Some(error(id, -32601, &format!("method not found: {method}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[derive(Deserialize)]
struct ListArgs {
    #[serde(default)]
    status: Status,
}

#[derive(Deserialize)]
struct AddArgs {
    items: Vec<String>,
}

#[derive(Deserialize)]
struct SetDoneArgs {
    ids: Vec<u64>,
    done: Option<bool>,
}

#[derive(Deserialize)]
struct EditArgs {
    id: u64,
    text: String,
}

#[derive(Deserialize)]
struct RemoveArgs {
    ids: Vec<u64>,
}

/// Runs a tool and returns the affected items as pretty JSON.
fn call(lists: &Lists, default: &Store, name: &str, args: Value) -> Result<String> {
    if name == "list_checklists" {
        return Ok(serde_json::to_string_pretty(&lists.summaries()?)?);
    }
    let named = match args.get("list") {
        None | Some(Value::Null) => None,
        Some(Value::String(list)) => Some(lists.open(list)?),
        Some(_) => bail!("invalid arguments: list must be a string"),
    };
    let store = named.as_ref().unwrap_or(default);
    let items = match name {
        "list_items" => {
            let a: ListArgs = parse(args)?;
            store.load()?.filtered(a.status)
        }
        "add_items" => {
            let a: AddArgs = parse(args)?;
            if a.items.is_empty() {
                bail!("items is empty");
            }
            store.update(|l| a.items.iter().map(|t| l.add(t)).collect())?
        }
        "set_done" => {
            let a: SetDoneArgs = parse(args)?;
            store.update(|l| l.set_done(&a.ids, a.done.unwrap_or(true)))?
        }
        "edit_item" => {
            let a: EditArgs = parse(args)?;
            vec![store.update(|l| l.edit(a.id, &a.text))?]
        }
        "remove_items" => {
            let a: RemoveArgs = parse(args)?;
            store.update(|l| l.remove(&a.ids))?
        }
        "clear_done" => store.update(|l| Ok(l.clear_done()))?,
        _ => bail!("unknown tool: {name}"),
    };
    Ok(serde_json::to_string_pretty(&items)?)
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T> {
    serde_json::from_value(args).context("invalid arguments")
}

fn tools(default_list: &str) -> Value {
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    let mut tools = json!([
        {
            "name": "list_items",
            "description": "List checklist items in display order. Each item has an id, its text and a done flag.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["all", "pending", "done"], "description": "Which items to return (default all)" },
                },
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "add_items",
            "description": "Append items to the end of the checklist. Returns the new items with their ids.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "items": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "Text of each item to add" },
                },
                "required": ["items"],
            },
        },
        {
            "name": "set_done",
            "description": "Check items off, or uncheck them with done=false. Fails without changing anything if any id is unknown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ids": ids,
                    "done": { "type": "boolean", "description": "true to check (default), false to uncheck" },
                },
                "required": ["ids"],
            },
        },
        {
            "name": "edit_item",
            "description": "Replace an item's text.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer" }, "text": { "type": "string" } },
                "required": ["id", "text"],
            },
        },
        {
            "name": "remove_items",
            "description": "Delete items. Fails without changing anything if any id is unknown.",
            "inputSchema": { "type": "object", "properties": { "ids": ids }, "required": ["ids"] },
            "annotations": { "destructiveHint": true },
        },
        {
            "name": "clear_done",
            "description": "Delete every checked item. Returns what was removed.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "destructiveHint": true },
        },
    ]);
    // Every item tool can target any list.
    let list = json!({
        "type": "string",
        "description": format!("Name of the checklist, e.g. \"auth-refactor\" (default \"{default_list}\"). A new name creates the list on first write."),
    });
    let items = tools.as_array_mut().expect("tools is a JSON array");
    for tool in items.iter_mut() {
        tool["inputSchema"]["properties"]["list"] = list.clone();
    }
    items.push(json!({
        "name": "list_checklists",
        "description": "List every checklist by name, with how many of its items are done.",
        "inputSchema": { "type": "object", "properties": {} },
        "annotations": { "readOnlyHint": true },
    }));
    tools
}
