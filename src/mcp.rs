//! `yap mcp`: an MCP server (JSON-RPC over stdio) that an AI app starts. It relays
//! tool calls to the running yap over a private local socket, so the AI works with
//! your live chats. yap only listens while *Settings → AI tools* is on.
//!
//! Reading needs `read` access; changing things needs `full`. Messages to a partner
//! are never sent without you saying yes in yap.

use crate::config::Paths;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

const PROTOCOL: &str = "2025-06-18";

/// One tool: its name, what it's for, whether it changes anything, and its arguments.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub writes: bool,
    /// `(name, type, description, required)`.
    pub args: &'static [(&'static str, &'static str, &'static str, bool)],
}

const CHAT: (&str, &str, &str, bool) =
    ("chat", "integer", "Chat tab number as shown in yap (1, 2, ...). Defaults to the one on screen.", false);

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "list_chats",
        description: "The open chat tabs: number, profile, connection, partner (if any) and unseen messages.",
        writes: false,
        args: &[],
    },
    Tool {
        name: "read_chat",
        description: "Messages in a chat tab, oldest first: who said it, when, and the text.",
        writes: false,
        args: &[CHAT, ("last", "integer", "How many recent entries (default 50).", false)],
    },
    Tool {
        name: "get_partner",
        description: "The current partner: gender, species, role, language, nickname, kinks (shared / theirs only / \
                      yours only, each with a definition), how long you've been chatting and when they last spoke.",
        writes: false,
        args: &[CHAT],
    },
    Tool {
        name: "get_profile",
        description: "Your active profile (preferences and character name) and the names of all profiles.",
        writes: false,
        args: &[CHAT],
    },
    Tool {
        name: "list_logs",
        description: "Earlier conversations in yap's Logs, newest first, optionally filtered by text.",
        writes: false,
        args: &[("query", "string", "Only chats matching this (title, partner, date or anything said).", false)],
    },
    Tool {
        name: "read_log",
        description: "A conversation from list_logs, by its index.",
        writes: false,
        args: &[
            ("index", "integer", "The index from list_logs.", true),
            ("last", "integer", "Only the last N entries.", false),
        ],
    },
    Tool {
        name: "get_history",
        description: "Partner history: patterns (how chats end, most-met species, length by shared kinks) and the most \
                      recent partners.",
        writes: false,
        args: &[("limit", "integer", "How many recent partners (default 20).", false)],
    },
    Tool {
        name: "get_stats",
        description: "Chat statistics, and when the site is busiest and matches come quickest.",
        writes: false,
        args: &[],
    },
    Tool {
        name: "define_kinks",
        description: "Short definitions of the site's kinks.",
        writes: false,
        args: &[("names", "array", "Kink names; leave out for all of them.", false)],
    },
    Tool {
        name: "draft_message",
        description: "Put text in a chat's message box for the user to edit and send. Nothing is sent.",
        writes: true,
        args: &[
            CHAT,
            ("text", "string", "The draft.", true),
            ("append", "boolean", "Add to the draft rather than replace it.", false),
        ],
    },
    Tool {
        name: "send_message",
        description: "Ask to send a message to the partner. yap shows it to the user, who approves or declines; this \
                      waits for their answer.",
        writes: true,
        args: &[CHAT, ("text", "string", "The message.", true)],
    },
    Tool {
        name: "find_partner",
        description: "Start looking for a partner with the chat's profile. Use next_partner to replace a current one.",
        writes: true,
        args: &[CHAT],
    },
    Tool {
        name: "next_partner",
        description: "Leave the current partner and look for a new one.",
        writes: true,
        args: &[CHAT],
    },
    Tool { name: "leave_partner", description: "Disconnect from the current partner.", writes: true, args: &[CHAT] },
    Tool {
        name: "block_partner",
        description: "Block the current (or just-left) partner.",
        writes: true,
        args: &[CHAT],
    },
    Tool {
        name: "set_nickname",
        description: "Call the current partner something in yap (display only; they never see it).",
        writes: true,
        args: &[CHAT, ("name", "string", "The nickname; empty clears it.", true)],
    },
    Tool {
        name: "switch_profile",
        description: "Switch a chat tab to another preference profile.",
        writes: true,
        args: &[CHAT, ("name", "string", "Profile name.", true)],
    },
    Tool {
        name: "add_snippet",
        description: "Save reusable text as a snippet (inserted with ;name then Tab). {species} etc. are filled in.",
        writes: true,
        args: &[("name", "string", "Snippet name.", true), ("text", "string", "Snippet text.", true)],
    },
    Tool {
        name: "save_link",
        description: "Save a link to yap's drawer.",
        writes: true,
        args: &[("url", "string", "The link.", true), ("label", "string", "A label; #tags become tags.", false)],
    },
];

pub fn tool(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

fn schema(tool: &Tool) -> Value {
    let mut props = serde_json::Map::new();
    for (name, ty, description, _) in tool.args {
        let mut prop = json!({ "type": ty, "description": description });
        if *ty == "array" {
            prop["items"] = json!({ "type": "string" });
        }
        props.insert((*name).to_owned(), prop);
    }
    let required: Vec<&str> = tool.args.iter().filter(|a| a.3).map(|a| a.0).collect();
    json!({ "type": "object", "properties": props, "required": required })
}

/// Where the running yap listens: the user's runtime folder, or a private folder in
/// yap's data folder.
pub fn socket_path(paths: &Paths) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("yap.sock"),
        _ => paths.data_dir.join("run").join("yap.sock"),
    }
}

/// How a tool call reaches the running yap.
pub trait Relay {
    fn call(&mut self, tool: &str, args: &Value) -> Result<Value, String>;
}

/// Answer one JSON-RPC message. Notifications get no reply.
pub fn handle(message: &Value, relay: &mut dyn Relay) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL);
            Ok(json!({
                "protocolVersion": asked,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "yap", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Tools for yap, a YiffSpot chat client. Chat numbers match yap's tabs. Messages to a \
                                 partner go through send_message, which the user approves in yap; draft_message \
                                 leaves text for them to edit instead.",
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let tools: Vec<Value> = TOOLS
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": schema(t) }))
                .collect();
            Ok(json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (text, error) = match tool(name) {
                None => (format!("There's no tool called {name}."), true),
                Some(_) => match relay.call(name, &args) {
                    Ok(value) => (serde_json::to_string_pretty(&value).unwrap_or_default(), false),
                    Err(e) => (e, true),
                },
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": error }))
        }
        other => Err(json!({ "code": -32601, "message": format!("unknown method {other}") })),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    })
}

/// The wire format between `yap mcp` and the running yap: one JSON object per line.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Request {
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Reply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Relays over the running yap's socket, connecting afresh for each call.
#[cfg(unix)]
pub struct SocketRelay {
    pub path: PathBuf,
}

#[cfg(unix)]
impl Relay for SocketRelay {
    fn call(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
        use std::os::unix::net::UnixStream;
        let mut stream = UnixStream::connect(&self.path).map_err(|_| {
            "yap isn't running with AI tools on. Start yap and set Settings → AI tools to read or full.".to_owned()
        })?;
        // Long enough for the user to read and approve a message.
        stream.set_read_timeout(Some(std::time::Duration::from_secs(6 * 60))).map_err(|e| e.to_string())?;
        let mut line =
            serde_json::to_string(&Request { tool: tool.to_owned(), args: args.clone() }).map_err(|e| e.to_string())?;
        line.push('\n');
        stream.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        let mut answer = String::new();
        BufReader::new(stream).read_line(&mut answer).map_err(|e| format!("no answer from yap: {e}"))?;
        let reply: Reply = serde_json::from_str(&answer).map_err(|e| format!("odd answer from yap: {e}"))?;
        match (reply.ok, reply.error) {
            (_, Some(e)) => Err(e),
            (Some(v), None) => Ok(v),
            (None, None) => Ok(Value::Null),
        }
    }
}

/// Run the MCP server on stdin/stdout until the client hangs up.
pub fn serve(relay: &mut dyn Relay) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(&message, relay),
            Err(e) => {
                Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } }))
            }
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Vec<(String, Value)>);
    impl Relay for Fake {
        fn call(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
            self.0.push((tool.to_owned(), args.clone()));
            if tool == "get_stats" { Err("AI tools are off in yap".into()) } else { Ok(json!({ "ok": true })) }
        }
    }

    #[test]
    fn speaks_mcp() {
        let mut relay = Fake(Vec::new());
        let init = handle(
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
            &mut relay,
        )
        .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "yap");
        assert!(handle(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}), &mut relay).is_none());

        let list = handle(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), &mut relay).unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), TOOLS.len());
        let send = tools.iter().find(|t| t["name"] == "send_message").unwrap();
        assert_eq!(send["inputSchema"]["required"], json!(["text"]));

        let call =
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_chat","arguments":{"last":5}}});
        let result = handle(&call, &mut relay).unwrap();
        assert_eq!(result["result"]["isError"], false);
        assert_eq!(relay.0, [("read_chat".to_owned(), json!({"last": 5}))]);

        let off =
            handle(&json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_stats"}}), &mut relay)
                .unwrap();
        assert_eq!(off["result"]["isError"], true);
        let unknown =
            handle(&json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}), &mut relay)
                .unwrap();
        assert_eq!(unknown["result"]["isError"], true);
        let bad = handle(&json!({"jsonrpc":"2.0","id":6,"method":"resources/list"}), &mut relay).unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }

    #[test]
    fn every_write_tool_is_marked() {
        for name in ["draft_message", "send_message", "next_partner", "switch_profile"] {
            assert!(tool(name).unwrap().writes, "{name}");
        }
        for name in ["read_chat", "list_logs", "get_partner"] {
            assert!(!tool(name).unwrap().writes, "{name}");
        }
    }
}
