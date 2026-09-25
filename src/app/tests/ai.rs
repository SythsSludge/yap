//! Tools for AI apps (yap mcp).

use super::*;
use crate::app::AiOutcome;
use crate::config::AiAccess;
use pretty_assertions::assert_eq;
use serde_json::json;

fn done(outcome: AiOutcome) -> Result<serde_json::Value, String> {
    match outcome {
        AiOutcome::Done(r) => r,
        AiOutcome::Waiting(id) => panic!("waiting on {id}"),
    }
}

#[test]
fn access_levels_gate_the_tools() {
    let mut h = harness().online().with_prefs().partnered();
    assert!(done(h.ai_call("read_chat", &json!({}))).unwrap_err().contains("off"));
    h.config.settings.ai_access = AiAccess::Read;
    assert!(done(h.ai_call("read_chat", &json!({}))).is_ok());
    assert!(done(h.ai_call("draft_message", &json!({"text": "hi"}))).unwrap_err().contains("only lets the AI read"));
    h.config.settings.ai_access = AiAccess::Full;
    assert!(done(h.ai_call("draft_message", &json!({"text": "hi"}))).is_ok());
    assert!(h.ai_active());
    assert!(done(h.ai_call("read_chat", &json!({"chat": 9}))).unwrap_err().contains("no chat 9"));
}

#[test]
fn reading_chats_partners_and_profiles() {
    let mut h = harness().online().with_prefs().partnered();
    h.config.settings.ai_access = AiAccess::Read;
    h.server(ServerMessage::ReceiveMessage("hey *waves*".into()));
    let chat = done(h.ai_call("read_chat", &json!({"last": 1}))).unwrap();
    assert_eq!(chat["entries"].as_array().unwrap().len(), 1);
    assert_eq!(chat["entries"][0]["from"], "partner");
    assert_eq!(chat["entries"][0]["text"], "hey *waves*");

    let partner = done(h.ai_call("get_partner", &json!({}))).unwrap();
    assert_eq!(partner["species"], "Fox");
    assert_eq!(partner["kinks"]["shared"][0]["kink"], "Musk");
    assert_eq!(partner["kinks"]["shared"][0]["means"], crate::glossary::define("Musk").unwrap());

    let chats = done(h.ai_call("list_chats", &json!({}))).unwrap();
    assert_eq!(chats["chats"][0]["partner"]["species"], "Fox");
    let profile = done(h.ai_call("get_profile", &json!({}))).unwrap();
    assert_eq!(profile["profile"], "default");
    let kinks = done(h.ai_call("define_kinks", &json!({"names": ["Biting"]}))).unwrap();
    assert_eq!(kinks["Biting"], crate::glossary::define("Biting").unwrap());
    assert!(done(h.ai_call("get_history", &json!({}))).is_ok());
    assert!(done(h.ai_call("get_stats", &json!({}))).is_ok());
    assert!(done(h.ai_call("list_logs", &json!({}))).unwrap()["chats"].as_array().unwrap().len() == 1);
    assert!(done(h.ai_call("read_log", &json!({"index": 0}))).is_ok());
}

#[test]
fn sending_waits_for_your_yes() {
    let mut h = harness().online().with_prefs().partnered();
    h.config.settings.ai_access = AiAccess::Full;
    h.take_effects();
    let AiOutcome::Waiting(id) = h.ai_call("send_message", &json!({"text": "hello from the AI"})) else { panic!() };
    assert!(matches!(&h.modal, Some(Modal::AiSend(s)) if s.text == "hello from the AI"));
    assert!(h.sent().is_empty(), "nothing goes out before you answer");
    h.press(KeyCode::Char('y'));
    let effects = h.take_effects();
    assert!(effects.contains(&Effect::Send(ClientMessage::SendMessage("hello from the AI".into()))));
    assert!(effects.contains(&Effect::AiResolved { id, result: Ok(json!({"sent": "hello from the AI"})) }));

    // Declining tells the AI; e takes it into the message box instead.
    let AiOutcome::Waiting(id) = h.ai_call("send_message", &json!({"text": "second"})) else { panic!() };
    h.press(KeyCode::Char('n'));
    assert_eq!(h.take_effects(), vec![Effect::AiResolved { id, result: Err("the user declined to send it".into()) }]);
    let AiOutcome::Waiting(_) = h.ai_call("send_message", &json!({"text": "third"})) else { panic!() };
    h.press(KeyCode::Char('e'));
    assert_eq!(h.input.text(), "third");

    // Two at once queue up, one popup at a time.
    h.input.clear();
    h.ai_call("send_message", &json!({"text": "a"}));
    h.ai_call("send_message", &json!({"text": "b"}));
    h.press(KeyCode::Char('n'));
    h.on_tick();
    assert!(matches!(&h.modal, Some(Modal::AiSend(s)) if s.text == "b"));
}

#[test]
fn acting_on_chats() {
    let mut h = harness().online().with_prefs().partnered();
    h.config.settings.ai_access = AiAccess::Full;
    done(h.ai_call("draft_message", &json!({"text": "draft"}))).unwrap();
    done(h.ai_call("draft_message", &json!({"text": "more", "append": true}))).unwrap();
    assert_eq!(h.input.text(), "draft more");
    assert!(h.last_toast().unwrap().starts_with("AI put a draft"));
    done(h.ai_call("set_nickname", &json!({"name": "Vix"}))).unwrap();
    assert_eq!(h.partner_nick.as_deref(), Some("Vix"));
    done(h.ai_call("add_snippet", &json!({"name": "hi", "text": "Hello!"}))).unwrap();
    assert!(h.drawer.snippet("hi").is_some());
    done(h.ai_call("save_link", &json!({"url": "https://e621.net/posts/1", "label": "ref"}))).unwrap();
    assert_eq!(h.drawer.items.len(), 1);
    h.take_effects();
    done(h.ai_call("next_partner", &json!({}))).unwrap();
    assert!(find_payload(&h.sent()).is_some());
    assert!(done(h.ai_call("leave_partner", &json!({}))).is_err(), "no partner while searching");

    // Other chats by number.
    h.run_command(Command::NewChat);
    h.on_net_for(1, NetEvent::Open);
    h.switch_session(0);
    done(h.ai_call("draft_message", &json!({"chat": 2, "text": "for chat two"}))).unwrap();
    assert_eq!(h.input.text(), "draft more", "the chat on screen is untouched");
    h.switch_session(1);
    assert_eq!(h.input.text(), "for chat two");
}
