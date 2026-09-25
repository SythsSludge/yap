//! The tools an AI app can use through `yap mcp` (see `mcp.rs`). Reading needs
//! *Settings → AI tools* at `read`; anything else needs `full`. A message to a partner
//! is only sent once you say yes here.

use super::*;
use crate::config::AiAccess;
use serde_json::{Value, json};

/// What became of a tool call.
#[derive(Debug, Clone, PartialEq)]
pub enum AiOutcome {
    Done(Result<Value, String>),
    /// Waiting on the user (see [`Effect::AiResolved`]).
    Waiting(u64),
}

/// A message the AI wants to send, waiting for your yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiSend {
    pub id: u64,
    pub chat: u64,
    pub text: String,
}

fn arg_str<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str)
}

fn need_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    arg_str(args, name).ok_or_else(|| format!("`{name}` is required"))
}

fn arg_usize(args: &Value, name: &str) -> Option<usize> {
    args.get(name).and_then(Value::as_u64).map(|n| n as usize)
}

fn entry_json(e: &chat::Entry, you: &str, partner: &str) -> Value {
    let (from, text) = match &e.kind {
        EntryKind::You(t) => (you.to_owned(), t.clone()),
        EntryKind::Partner(t) => (partner.to_owned(), t.clone()),
        EntryKind::System(t) => ("system".to_owned(), t.clone()),
        EntryKind::Warning(t) => ("warning".to_owned(), t.clone()),
        EntryKind::PartnerInfo { info, .. } => ("system".to_owned(), chat::partner_summary(info)),
    };
    json!({ "time": e.at.format("%Y-%m-%d %H:%M").to_string(), "from": from, "text": text })
}

fn last_n<T>(items: &[T], n: usize) -> &[T] {
    &items[items.len().saturating_sub(n)..]
}

impl App {
    /// Run a tool for the AI app. Calls that change something show a toast, so you
    /// always know what it did.
    pub fn ai_call(&mut self, tool: &str, args: &Value) -> AiOutcome {
        let access = self.config.settings.ai_access;
        let Some(spec) = crate::mcp::tool(tool) else {
            return AiOutcome::Done(Err(format!("unknown tool {tool}")));
        };
        if access == AiAccess::Off {
            return AiOutcome::Done(Err("AI tools are off in yap (Settings → AI tools).".into()));
        }
        if spec.writes && access != AiAccess::Full {
            return AiOutcome::Done(Err("yap only lets the AI read right now (Settings → AI tools: full).".into()));
        }
        self.ai_seen = Some(self.now);
        let chat = match self.ai_chat(args) {
            Ok(id) => id,
            Err(e) => return AiOutcome::Done(Err(e)),
        };
        if tool == "send_message" {
            return match need_str(args, "text") {
                Ok(text) => self.ai_request_send(chat, text),
                Err(e) => AiOutcome::Done(Err(e)),
            };
        }
        let result = match tool {
            "list_chats" => Ok(self.ai_list_chats()),
            "read_chat" => Ok(self
                .with_session(chat, |app| app.ai_read_chat(arg_usize(args, "last").unwrap_or(50)))
                .unwrap_or_default()),
            "get_partner" => Ok(self.with_session(chat, App::ai_partner).unwrap_or_default()),
            "get_profile" => Ok(self.with_session(chat, App::ai_profile).unwrap_or_default()),
            "list_logs" => Ok(self.ai_list_logs(arg_str(args, "query").unwrap_or_default())),
            "read_log" => self.ai_read_log(args),
            "get_history" => Ok(self.ai_history(arg_usize(args, "limit").unwrap_or(20))),
            "get_stats" => Ok(self.ai_stats()),
            "define_kinks" => Ok(ai_kinks(args)),
            _ => self
                .with_session(chat, |app| app.ai_act(tool, args))
                .unwrap_or_else(|| Err("that chat just closed".into())),
        };
        AiOutcome::Done(result)
    }

    /// The session a call is about: `chat` (as numbered in yap) or the one on screen.
    fn ai_chat(&self, args: &Value) -> Result<u64, String> {
        match args.get("chat").and_then(Value::as_u64) {
            None => Ok(self.session_id),
            Some(n) => self
                .sessions()
                .into_iter()
                .find(|s| s.number as u64 == n)
                .map(|s| s.id)
                .ok_or_else(|| format!("there's no chat {n} (see list_chats)")),
        }
    }

    fn ai_list_chats(&self) -> Value {
        let partners: HashMap<u64, Option<PartnerInfo>> = self
            .others
            .iter()
            .map(|s| (s.id, s.partner.clone()))
            .chain([(self.session_id, self.partner.clone())])
            .map(|(id, p)| (id, if let PartnerState::Connected(i) = p { Some(i) } else { None }))
            .collect();
        let chats: Vec<Value> = self
            .sessions()
            .into_iter()
            .map(|s| {
                json!({
                    "chat": s.number,
                    "on_screen": s.active,
                    "profile": s.profile,
                    "status": s.label,
                    "online": s.online,
                    "unseen": s.unseen,
                    "partner": partners.get(&s.id).cloned().flatten(),
                })
            })
            .collect();
        json!({ "chats": chats })
    }

    fn ai_read_chat(&mut self, last: usize) -> Value {
        let (you, partner) = (self.my_label(), self.partner_label());
        let entries: Vec<Value> =
            last_n(&self.chat.entries, last).iter().map(|e| entry_json(e, &you, &partner)).collect();
        json!({
            "you": you,
            "partner": partner,
            "partner_typing": self.chat.partner_typing,
            "entries": entries,
            "draft": self.input.text(),
        })
    }

    fn ai_partner(&mut self) -> Value {
        let PartnerState::Connected(info) = self.partner.clone() else {
            return json!({ "connected": false, "searching": self.partner == PartnerState::Searching });
        };
        let groups = self.kink_groups().unwrap_or_default();
        let explain = |kinks: &[String]| -> Vec<Value> {
            kinks.iter().map(|k| json!({ "kink": k, "means": crate::glossary::define(k) })).collect()
        };
        let secs = |t: Option<Instant>| t.map(|t| self.now.saturating_duration_since(t).as_secs());
        let (their_words, your_words) = self.chat.average_words();
        json!({
            "connected": true,
            "gender": info.gender,
            "species": info.species,
            "role": info.role,
            "language": info.language,
            "nickname": self.partner_nick,
            "kinks": {
                "shared": explain(&groups.shared),
                "theirs_only": explain(&groups.theirs),
                "yours_only": explain(&groups.mine),
            },
            "chatting_for_secs": secs(self.clock.partner_since),
            "last_spoke_secs_ago": secs(self.clock.last_heard),
            "typing": self.chat.partner_typing,
            "average_words": { "them": their_words, "you": your_words },
        })
    }

    fn ai_profile(&mut self) -> Value {
        let p = self.config.active();
        let prefs: serde_json::Map<String, Value> =
            crate::prefs::Field::ALL.iter().map(|f| (f.label().to_owned(), json!(p.preferences.summary(*f)))).collect();
        json!({
            "profile": p.name,
            "character": p.character,
            "preferences": prefs,
            "all_profiles": self.config.profiles.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        })
    }

    fn ai_list_logs(&mut self, query: &str) -> Value {
        if query.chars().count() >= 2 {
            self.logs.load_all();
        }
        let q = query.to_lowercase();
        let chats: Vec<Value> = (0..self.logs.items.len())
            .rev()
            .filter(|&i| self.logs.items[i].matches(&q))
            .map(|i| {
                let c = &self.logs.items[i];
                json!({
                    "index": i,
                    "title": c.title(),
                    "partner": c.partner_title(),
                    "started": c.started.format("%Y-%m-%d %H:%M").to_string(),
                    "ended": c.ended.map(|e| e.format("%Y-%m-%d %H:%M").to_string()),
                    "messages": c.messages,
                    "pinned": c.pinned,
                })
            })
            .collect();
        json!({ "chats": chats })
    }

    fn ai_read_log(&mut self, args: &Value) -> Result<Value, String> {
        let index = arg_usize(args, "index").ok_or("`index` is required (see list_logs)")?;
        if index >= self.logs.items.len() {
            return Err(format!("there's no log {index}"));
        }
        let entries = self.logs.open(index).map_err(|e| format!("{e:#}"))?.to_vec();
        let conv = &self.logs.items[index];
        let (you, partner) = conv.names();
        let shown = last_n(&entries, arg_usize(args, "last").unwrap_or(usize::MAX));
        Ok(json!({
            "title": conv.title(),
            "entries": shown.iter().map(|e| entry_json(e, &you, &partner)).collect::<Vec<_>>(),
        }))
    }

    fn ai_history(&self, limit: usize) -> Value {
        let s = self.history.summary();
        json!({
            "met": s.met,
            "auto_skipped": s.skipped,
            "average_chat_secs": s.average_secs,
            "chats_where_nobody_spoke": s.silent,
            "how_chats_ended": s.outcomes.iter().map(|(o, n)| json!({ "outcome": o.describe(), "count": n })).collect::<Vec<_>>(),
            "most_met_species": s.species.iter().map(|(name, n, secs)| json!({ "species": name, "chats": n, "average_secs": secs })).collect::<Vec<_>>(),
            "average_secs_by_shared_kinks": { "none": s.by_shared[0].1, "one_or_two": s.by_shared[1].1, "three_or_more": s.by_shared[2].1 },
            "recent": last_n(&self.history.records, limit).iter().rev().collect::<Vec<_>>(),
        })
    }

    fn ai_stats(&self) -> Value {
        let quick: Vec<Value> = self
            .activity
            .quickest(3, 5)
            .iter()
            .map(|q| json!({ "hour": q.hour, "average_wait_secs": q.wait_secs, "searches": q.searches }))
            .collect();
        json!({
            "this_session": self.run_stats,
            "all_time": self.stats,
            "average_online_by_hour": self.activity.online_by_hour(),
            "quickest_hours": quick,
        })
    }

    /// The tools that change things, run with the chat swapped in.
    fn ai_act(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
        let done = |what: String| Ok(json!({ "done": what }));
        match tool {
            "draft_message" => {
                let text = need_str(args, "text")?;
                if args.get("append").and_then(Value::as_bool) == Some(true) {
                    self.input.end();
                    self.insert_into_input(text);
                } else {
                    self.input.set(text);
                    self.input_changed();
                }
                self.ai_note("put a draft in your message box");
                done("drafted; the user can edit and send it".into())
            }
            "find_partner" => {
                if self.has_partner() {
                    return Err("already chatting; use next_partner to move on".into());
                }
                self.request_find();
                self.ai_note("started looking for a partner");
                done("searching".into())
            }
            "next_partner" => {
                self.next_partner();
                self.ai_note("moved on to a new partner");
                done("searching for someone new".into())
            }
            "leave_partner" => {
                if !self.has_partner() {
                    return Err("no partner to leave".into());
                }
                self.run_confirmed(Confirm::Leave);
                self.ai_note("left your partner");
                done("left".into())
            }
            "block_partner" => {
                if !self.has_partner() && !self.can_block_previous {
                    return Err("no partner to block".into());
                }
                self.run_confirmed(Confirm::Block);
                self.ai_note("blocked your partner");
                done("blocked".into())
            }
            "set_nickname" => {
                if !self.has_partner() {
                    return Err("no partner to nickname".into());
                }
                let name = arg_str(args, "name").unwrap_or_default();
                self.set_nick(name);
                self.ai_note(&format!("nicknamed your partner `{name}`"));
                done("nickname set".into())
            }
            "switch_profile" => {
                let name = need_str(args, "name")?;
                if !self.config.profiles.iter().any(|p| p.name == name) {
                    return Err(format!("there's no profile called {name}"));
                }
                self.switch_profile(name);
                done(format!("using profile {name}"))
            }
            "add_snippet" => {
                let (name, text) = (need_str(args, "name")?, need_str(args, "text")?);
                self.add_snippet(name, text).ok_or("couldn't save that snippet (name taken or invalid?)")?;
                done(format!("saved snippet {name}"))
            }
            "save_link" => {
                let url = need_str(args, "url")?;
                let before = self.drawer.items.len();
                self.add_to_drawer(url, arg_str(args, "label").unwrap_or_default());
                if self.drawer.items.len() == before {
                    return Err("couldn't save that link".into());
                }
                done("saved to the drawer".into())
            }
            other => Err(format!("unknown tool {other}")),
        }
    }

    fn ai_note(&mut self, what: &str) {
        let chat = if self.session_count() > 1 { format!(" (chat {})", self.session_number()) } else { String::new() };
        self.toast(Level::Info, format!("AI {what}{chat}."));
    }

    /// `send_message`: queue it for your approval.
    fn ai_request_send(&mut self, chat: u64, text: &str) -> AiOutcome {
        let text = crate::text::sanitize(text.trim());
        if text.is_empty() {
            return AiOutcome::Done(Err("`text` is empty".into()));
        }
        let ready = self.with_session(chat, |app| app.has_partner() && app.is_online()).unwrap_or(false);
        if !ready {
            return AiOutcome::Done(Err("that chat has no partner to send to".into()));
        }
        self.next_ai_id += 1;
        let id = self.next_ai_id;
        self.ai_sends.push_back(AiSend { id, chat, text });
        self.show_next_ai_send();
        AiOutcome::Waiting(id)
    }

    /// Put the next waiting message up for approval, when nothing else is open.
    pub(super) fn show_next_ai_send(&mut self) {
        if self.modal.is_some() || self.viewer.is_some() {
            return;
        }
        if let Some(send) = self.ai_sends.pop_front() {
            self.modal = Some(Modal::AiSend(send));
            self.alert("AI wants to send a message");
        }
    }

    /// Your answer to a message the AI wanted to send.
    pub(super) fn answer_ai_send(&mut self, send: AiSend, approved: bool) {
        let result = if !approved {
            Err("the user declined to send it".to_owned())
        } else {
            let sent = self.with_session(send.chat, |app| {
                if !app.has_partner() || !app.is_online() {
                    return Err("the partner is gone".to_owned());
                }
                app.send_chat(send.text.clone());
                Ok(json!({ "sent": send.text }))
            });
            sent.unwrap_or_else(|| Err("that chat closed".into()))
        };
        self.effect(Effect::AiResolved { id: send.id, result });
    }

    /// Whether an AI app used yap in the last minute (for the header).
    pub fn ai_active(&self) -> bool {
        self.ai_seen.is_some_and(|t| self.now.saturating_duration_since(t) < Duration::from_secs(60))
    }
}

fn ai_kinks(args: &Value) -> Value {
    let wanted: Vec<String> = args
        .get("names")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_else(|| crate::catalog::KINKS.iter().map(|k| (*k).to_owned()).collect());
    let defs: serde_json::Map<String, Value> =
        wanted.into_iter().map(|k| (k.clone(), json!(crate::glossary::define(&k)))).collect();
    Value::Object(defs)
}
