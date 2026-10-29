//! The local control protocol: newline-delimited JSON over TCP, bound to 127.0.0.1 only.
//!
//! Request:  `{"id": 1, "tool": "get_nets", "args": {...}, "agent": "claude-code"}`
//! Response: `{"id": 1, "ok": true, "result": ...}` or `{"id": 1, "ok": false, "error": {...}}`
//!
//! When the environment variable `HEPHAESTUS_TOKEN` is set on the server, every request must carry
//! the same value in `"token"`.
//!
//! The GUI runs a [`serve`] listener and answers [`Request`]s on its UI thread, so agent changes
//! appear immediately; [`serve_session`] answers them on its own thread for headless use. The MCP
//! server and the Python SDK are clients of this protocol.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use serde_json::{Value, json};

use crate::{ApiError, Session, tools};

/// The default port (Hephaestus's "PC" on a phone keypad, 7 2).
pub const DEFAULT_PORT: u16 = 7272;
const MAX_LINE: u64 = 16 * 1024 * 1024;

/// One request waiting for an answer.
pub struct Request {
    pub id: Value,
    pub tool: String,
    pub args: Value,
    pub agent: Option<String>,
    reply: Sender<Value>,
}

impl Request {
    pub fn respond(self, result: Result<Value, ApiError>) {
        let line = match result {
            Ok(result) => json!({"id": self.id, "ok": true, "result": result}),
            Err(error) => json!({"id": self.id, "ok": false, "error": error.to_json()}),
        };
        // The client may have disconnected; nothing else to do then.
        let _ = self.reply.send(line);
    }
}

fn error_line(id: Value, error: ApiError) -> String {
    json!({"id": id, "ok": false, "error": error.to_json()}).to_string()
}

fn handle_connection(stream: TcpStream, requests: Sender<Request>, wake: Arc<dyn Fn() + Send + Sync>, token: Option<String>) {
    let Ok(read_half) = stream.try_clone() else { return };
    let mut writer = stream;
    let mut reader = BufReader::new(read_half);
    loop {
        let mut line = String::new();
        let limited = (&mut reader).take(MAX_LINE).read_line(&mut line);
        match limited {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(e) => error_line(Value::Null, ApiError::new("parse_error", e.to_string())),
            Ok(msg) => {
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let tool = msg.get("tool").or_else(|| msg.get("method")).and_then(Value::as_str).unwrap_or_default().to_string();
                if token.as_deref().is_some_and(|t| msg.get("token").and_then(Value::as_str) != Some(t)) {
                    error_line(id, ApiError::new("unauthorized", "missing or wrong token"))
                } else if tool.is_empty() {
                    error_line(id, ApiError::invalid("missing tool"))
                } else {
                    let (tx, rx) = channel();
                    let request = Request {
                        id: id.clone(),
                        tool,
                        args: msg.get("args").or_else(|| msg.get("params")).cloned().unwrap_or(json!({})),
                        agent: msg.get("agent").and_then(Value::as_str).map(String::from).or_else(|| Some("agent".into())),
                        reply: tx,
                    };
                    if requests.send(request).is_err() {
                        return;
                    }
                    wake();
                    match rx.recv_timeout(Duration::from_secs(120)) {
                        Ok(v) => v.to_string(),
                        Err(_) => error_line(id, ApiError::new("timeout", "the application did not answer in time")),
                    }
                }
            }
        };
        if writer.write_all(reply.as_bytes()).and_then(|_| writer.write_all(b"\n")).and_then(|_| writer.flush()).is_err() {
            return;
        }
    }
}

/// Listen on 127.0.0.1:`port` (0 picks a free port). Requests arrive on the returned receiver;
/// `wake` is called after each one is queued (the GUI requests a repaint).
pub fn serve(port: u16, wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<(SocketAddr, Receiver<Request>)> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let addr = listener.local_addr()?;
    let (tx, rx) = channel();
    let token = access_token();
    std::thread::Builder::new().name("hephaestus-control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let wake = wake.clone();
            let token = token.clone();
            let _ = std::thread::Builder::new()
                .name("hephaestus-control-conn".into())
                .spawn(move || handle_connection(stream, tx, wake, token));
        }
    })?;
    Ok((addr, rx))
}

/// Answer requests against a session on the current thread until the listener stops.
pub fn serve_session(mut session: Session, port: u16, ready: impl FnOnce(SocketAddr)) -> std::io::Result<()> {
    let (addr, rx) = serve(port, Arc::new(|| {}))?;
    ready(addr);
    for request in rx {
        let result = tools::dispatch(&mut session, &request.tool, &request.args, request.agent.as_deref());
        request.respond(result);
    }
    Ok(())
}

/// A blocking client.
pub struct Client {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    next_id: u64,
    pub agent: String,
    pub token: Option<String>,
}

impl Client {
    pub fn connect(addr: &str, agent: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(Duration::from_secs(180)))?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self { writer: stream, reader, next_id: 1, agent: agent.into(), token: access_token() })
    }

    pub fn call(&mut self, tool: &str, args: Value) -> Result<Value, ApiError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut msg = json!({"id": id, "tool": tool, "args": args, "agent": self.agent});
        if let Some(t) = &self.token {
            msg["token"] = json!(t);
        }
        let io = |e: std::io::Error| ApiError::new("connection", e.to_string());
        self.writer.write_all(format!("{msg}\n").as_bytes()).map_err(io)?;
        let mut line = String::new();
        self.reader.read_line(&mut line).map_err(io)?;
        if line.is_empty() {
            return Err(ApiError::new("connection", "Hephaestus closed the connection"));
        }
        let reply: Value = serde_json::from_str(&line).map_err(|e| ApiError::new("parse_error", e.to_string()))?;
        if reply.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(reply.get("result").cloned().unwrap_or(Value::Null))
        } else {
            let e = reply.get("error").cloned().unwrap_or(Value::Null);
            Err(ApiError::new(
                e.get("code").and_then(Value::as_str).unwrap_or("error"),
                e.get("message").and_then(Value::as_str).unwrap_or("unknown error"),
            )
            .with_details(e.get("details").cloned().unwrap_or(Value::Null)))
        }
    }
}

/// The shared secret from `HEPHAESTUS_TOKEN` (or `PCBCRAFT_TOKEN`, its name before the rename).
fn access_token() -> Option<String> {
    ["HEPHAESTUS_TOKEN", "PCBCRAFT_TOKEN"].into_iter().find_map(|k| std::env::var(k).ok().filter(|t| !t.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_server_round_trip() {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = serve_session(Session::new("remote"), 0, |addr| {
                let _ = tx.send(addr);
            });
        });
        let addr = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut c = Client::connect(&addr.to_string(), "test-agent").unwrap();
        let info = c.call("get_project_info", json!({})).unwrap();
        assert_eq!(info["name"], "remote");
        let placed = c.call("add_component", json!({"part": "RES", "position": [25.4, 25.4]})).unwrap();
        assert_eq!(placed["status"], "committed");
        let err = c.call("no_such_tool", json!({})).unwrap_err();
        assert_eq!(err.code, "unknown_tool");
        let changes = c.call("get_changes", json!({"since": 0})).unwrap();
        assert_eq!(changes["changes"][0]["source"]["name"], "test-agent");
    }
}
