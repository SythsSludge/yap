//! The YiffSpot websocket protocol.
//!
//! Every frame in both directions is a JSON text frame shaped `{"type": ..., "data": ...}`.
//! Messages with no meaningful payload carry `"data": true`. The shapes here mirror
//! `vendor/yiffspot/src/server/*.js` and `src/client/js/index.js`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Preferences exactly as `find_partner` expects them on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WirePreferences {
    pub user: WireUser,
    pub partner: WirePartner,
    pub kinks: Vec<String>,
}

/// Field order matters: the server's `checkInvalid` only validates the *first* scalar
/// key of this object (it returns early), so `gender` must stay first to keep parity
/// with the web client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireUser {
    pub gender: String,
    pub species: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WirePartner {
    pub gender: Vec<String>,
    pub species: Vec<String>,
    pub role: String,
}

/// Frames the client sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMessage {
    FindPartner(WirePreferences),
    SendMessage(String),
    Typing(bool),
    BlockPartner,
    /// Leave the current partner without closing the socket.
    Disconnect,
    /// Application-level keepalive. The server only marks a socket alive when it
    /// receives a *message*, so websocket pings alone would get us terminated.
    Ping,
}

impl ClientMessage {
    pub fn kind(&self) -> &'static str {
        match self {
            ClientMessage::FindPartner(_) => "find_partner",
            ClientMessage::SendMessage(_) => "send_message",
            ClientMessage::Typing(_) => "typing",
            ClientMessage::BlockPartner => "block_partner",
            ClientMessage::Disconnect => "disconnect",
            ClientMessage::Ping => "ping",
        }
    }

    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct Envelope<'a, T: Serialize> {
            #[serde(rename = "type")]
            kind: &'a str,
            data: T,
        }
        let kind = self.kind();
        let result = match self {
            ClientMessage::FindPartner(prefs) => serde_json::to_string(&Envelope { kind, data: prefs }),
            ClientMessage::SendMessage(text) => serde_json::to_string(&Envelope { kind, data: text }),
            ClientMessage::Typing(on) => serde_json::to_string(&Envelope { kind, data: on }),
            ClientMessage::BlockPartner | ClientMessage::Disconnect | ClientMessage::Ping => {
                serde_json::to_string(&Envelope { kind, data: true })
            }
        };
        result.expect("client messages always serialize")
    }
}

/// What the server tells us about a newly matched partner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartnerInfo {
    pub gender: String,
    pub species: String,
    /// The server sends these pre-joined with `", "`.
    pub kinks: String,
    pub role: String,
    /// Only present for the user who initiated the match, and only on servers
    /// newer than the live site.
    #[serde(default)]
    pub language: Option<String>,
}

impl PartnerInfo {
    pub fn kink_list(&self) -> Vec<&str> {
        if self.kinks.is_empty() { Vec::new() } else { self.kinks.split(", ").collect() }
    }
}

/// Frames the server sends.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    ConnectionSuccess {
        token: String,
    },
    /// Another socket already holds our token; the server terminates this one.
    ConnectionExists,
    UserCount(u64),
    ReceiveMessage(String),
    PartnerTyping(bool),
    PartnerConnected(PartnerInfo),
    PartnerPending,
    PartnerLeft,
    PartnerDisconnected,
    PartnerBlocked,
    /// Acknowledges our own `disconnect` request.
    ClientDisconnect,
    InvalidPreferences,
    /// Anything we don't recognise. Kept rather than dropped so the traffic viewer
    /// and chat can surface protocol drift between the repo and the live site.
    Unknown {
        kind: String,
        data: Value,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("frame is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("frame has no string `type` field")]
    MissingType,
    #[error("`{kind}` frame has unexpected data: {data}")]
    BadData { kind: String, data: Value },
}

impl ServerMessage {
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut value: Value = serde_json::from_str(text)?;
        let kind = value.get("type").and_then(Value::as_str).ok_or(ParseError::MissingType)?.to_owned();
        let data = value.get_mut("data").map(Value::take).unwrap_or(Value::Null);
        let bad = |data: Value| ParseError::BadData { kind: kind.clone(), data };

        Ok(match kind.as_str() {
            "connection_success" => match data {
                Value::String(token) => ServerMessage::ConnectionSuccess { token },
                other => return Err(bad(other)),
            },
            "connection_exists" => ServerMessage::ConnectionExists,
            "update_user_count" => match data.as_u64() {
                Some(n) => ServerMessage::UserCount(n),
                // The server's counter can briefly go negative on odd disconnect orderings.
                None if data.as_i64().is_some() => ServerMessage::UserCount(0),
                None => return Err(bad(data)),
            },
            "receive_message" => match data {
                Value::String(text) => ServerMessage::ReceiveMessage(text),
                // The server relays whatever the partner sent, verbatim.
                other => ServerMessage::ReceiveMessage(other.to_string()),
            },
            "partner_typing" => ServerMessage::PartnerTyping(truthy(&data)),
            "partner_connected" => match serde_json::from_value(data.clone()) {
                Ok(info) => ServerMessage::PartnerConnected(info),
                Err(_) => return Err(bad(data)),
            },
            "partner_pending" => ServerMessage::PartnerPending,
            "partner_left" => ServerMessage::PartnerLeft,
            "partner_disconnected" => ServerMessage::PartnerDisconnected,
            "partner_blocked" => ServerMessage::PartnerBlocked,
            "client_disconnect" => ServerMessage::ClientDisconnect,
            "invalid_preferences" => ServerMessage::InvalidPreferences,
            _ => ServerMessage::Unknown { kind, data },
        })
    }
}

/// JavaScript-ish truthiness for the `partner_typing` payload, which is whatever
/// the partner's client put in its `typing` frame.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn sample_prefs() -> WirePreferences {
        WirePreferences {
            user: WireUser {
                gender: "Male".into(),
                species: "Wolf".into(),
                role: "Switch".into(),
                language: Some("any".into()),
            },
            partner: WirePartner {
                gender: vec!["any".into()],
                species: vec!["Fox".into(), "Dragon".into()],
                role: "Dominant".into(),
            },
            kinks: vec!["any".into()],
        }
    }

    #[test]
    fn client_messages_match_web_client_json() {
        let cases = [
            (ClientMessage::Ping, json!({"type": "ping", "data": true})),
            (ClientMessage::BlockPartner, json!({"type": "block_partner", "data": true})),
            (ClientMessage::Disconnect, json!({"type": "disconnect", "data": true})),
            (ClientMessage::Typing(false), json!({"type": "typing", "data": false})),
            (
                ClientMessage::SendMessage("hi <b>\"there\"".into()),
                json!({"type": "send_message", "data": "hi <b>\"there\""}),
            ),
        ];
        for (msg, expected) in cases {
            let actual: Value = serde_json::from_str(&msg.to_json()).unwrap();
            assert_eq!(actual, expected, "{msg:?}");
        }
    }

    #[test]
    fn find_partner_serializes_with_gender_first() {
        let json = ClientMessage::FindPartner(sample_prefs()).to_json();
        assert!(json.starts_with(r#"{"type":"find_partner","data":{"user":{"gender":"Male","#), "{json}");
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["data"]["partner"]["species"], json!(["Fox", "Dragon"]));
        assert_eq!(v["data"]["user"]["language"], json!("any"));
    }

    #[test]
    fn find_partner_can_omit_language_for_old_servers() {
        let mut prefs = sample_prefs();
        prefs.user.language = None;
        let json = ClientMessage::FindPartner(prefs).to_json();
        assert!(!json.contains("language"), "{json}");
    }

    #[test]
    fn parses_every_known_server_frame() {
        let cases: Vec<(&str, ServerMessage)> = vec![
            (
                r#"{"type":"connection_success","data":"1700000000000-abc"}"#,
                ServerMessage::ConnectionSuccess { token: "1700000000000-abc".into() },
            ),
            (r#"{"type":"connection_exists","data":true}"#, ServerMessage::ConnectionExists),
            (r#"{"type":"update_user_count","data":1234}"#, ServerMessage::UserCount(1234)),
            (r#"{"type":"update_user_count","data":-1}"#, ServerMessage::UserCount(0)),
            (r#"{"type":"receive_message","data":"hello"}"#, ServerMessage::ReceiveMessage("hello".into())),
            (r#"{"type":"partner_typing","data":true}"#, ServerMessage::PartnerTyping(true)),
            (r#"{"type":"partner_typing","data":false}"#, ServerMessage::PartnerTyping(false)),
            (r#"{"type":"partner_pending","data":true}"#, ServerMessage::PartnerPending),
            (r#"{"type":"partner_left","data":true}"#, ServerMessage::PartnerLeft),
            (r#"{"type":"partner_disconnected","data":true}"#, ServerMessage::PartnerDisconnected),
            (r#"{"type":"partner_blocked","data":true}"#, ServerMessage::PartnerBlocked),
            (r#"{"type":"client_disconnect","data":true}"#, ServerMessage::ClientDisconnect),
            (r#"{"type":"invalid_preferences","data":true}"#, ServerMessage::InvalidPreferences),
        ];
        for (raw, expected) in cases {
            assert_eq!(ServerMessage::parse(raw).unwrap(), expected, "{raw}");
        }
    }

    #[test]
    fn parses_partner_connected_with_and_without_language() {
        let with = r#"{"type":"partner_connected","data":{"gender":"Female","species":"Fox","kinks":"Anal, Biting","role":"Submissive","language":"English"}}"#;
        let ServerMessage::PartnerConnected(info) = ServerMessage::parse(with).unwrap() else {
            panic!("wrong variant");
        };
        assert_eq!(info.language.as_deref(), Some("English"));
        assert_eq!(info.kink_list(), vec!["Anal", "Biting"]);

        // The matched partner (not the searcher) gets no language field.
        let without =
            r#"{"type":"partner_connected","data":{"gender":"Male","species":"Wolf","kinks":"any","role":"Dominant"}}"#;
        let ServerMessage::PartnerConnected(info) = ServerMessage::parse(without).unwrap() else {
            panic!("wrong variant");
        };
        assert_eq!(info.language, None);
        assert_eq!(info.kink_list(), vec!["any"]);
    }

    #[test]
    fn partner_typing_uses_js_truthiness() {
        // A partner's client can send any JSON as the typing payload.
        for (data, expected) in [("1", true), ("0", false), ("\"\"", false), ("\"x\"", true), ("null", false)] {
            let raw = format!(r#"{{"type":"partner_typing","data":{data}}}"#);
            assert_eq!(ServerMessage::parse(&raw).unwrap(), ServerMessage::PartnerTyping(expected), "{raw}");
        }
    }

    #[test]
    fn non_string_messages_are_stringified_not_dropped() {
        let msg = ServerMessage::parse(r#"{"type":"receive_message","data":{"x":1}}"#).unwrap();
        assert_eq!(msg, ServerMessage::ReceiveMessage(r#"{"x":1}"#.into()));
    }

    #[test]
    fn unknown_types_are_preserved() {
        let msg = ServerMessage::parse(r#"{"type":"new_feature","data":[1,2]}"#).unwrap();
        assert_eq!(msg, ServerMessage::Unknown { kind: "new_feature".into(), data: json!([1, 2]) });
    }

    #[test]
    fn rejects_malformed_frames() {
        assert!(matches!(ServerMessage::parse("not json"), Err(ParseError::Json(_))));
        assert!(matches!(ServerMessage::parse(r#"{"data":1}"#), Err(ParseError::MissingType)));
        assert!(matches!(
            ServerMessage::parse(r#"{"type":"connection_success","data":5}"#),
            Err(ParseError::BadData { .. })
        ));
        assert!(matches!(
            ServerMessage::parse(r#"{"type":"partner_connected","data":{"gender":"x"}}"#),
            Err(ParseError::BadData { .. })
        ));
    }
}
