//! `hephaestus-mcp`: a Model Context Protocol server over stdio.
//!
//! Two backends share one tool catalog (`hephaestus_agent_api::tools`):
//! - **connected** (default when Hephaestus is running): forwards every call to the application's
//!   control port, so changes appear live in the open project and the user's edits are visible to
//!   the agent;
//! - **headless**: owns a session in this process (open a project with `--project`, or start
//!   empty or from the demo).
//!
//! ```text
//! hephaestus-mcp [--connect 127.0.0.1:7272] [--project path.pcbprj | --demo | --headless] [--agent NAME]
//! ```
//!
//! stdout carries the protocol; diagnostics go to stderr.

use std::sync::{Arc, Mutex};

use hephaestus_agent_api::control::{Client, DEFAULT_PORT};
use hephaestus_agent_api::{ApiError, Session, tools};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;

enum Backend {
    Remote(Mutex<Client>),
    Local(Box<Mutex<Session>>),
}

impl Backend {
    fn call(&self, tool: &str, args: Value, agent: &str) -> Result<Value, ApiError> {
        match self {
            Backend::Remote(client) => {
                let mut c = client.lock().map_err(|_| ApiError::new("internal", "connection lock poisoned"))?;
                c.call(tool, args)
            }
            Backend::Local(session) => {
                let mut s = session.lock().map_err(|_| ApiError::new("internal", "session lock poisoned"))?;
                // Headless sessions have nobody to approve proposals, so agents act directly.
                s.policy.approve_destructive = false;
                tools::dispatch(&mut s, tool, &args, Some(agent))
            }
        }
    }
}

#[derive(Clone)]
struct HephaestusMcp {
    backend: Arc<Backend>,
    agent: String,
    mode: String,
}

const INSTRUCTIONS: &str = "Hephaestus is an EDA application (schematic capture and PCB layout). \
Lengths are millimetres (numbers) or strings with units (\"100mil\"); results use millimetres. \
Typical flow: search_library → add_component → connect_pins/create_net → get_nets → run_erc → \
update_pcb → place_component → route_net → run_drc → export_gerber. Mutating tools accept dry_run, \
propose and require_clean. Destructive changes may come back as pending_approval when Hephaestus is \
running with a user; use get_changes to see what the user did.";

impl ServerHandler for HephaestusMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("hephaestus", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!("{INSTRUCTIONS} (backend: {})", self.mode))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = tools::catalog()
            .into_iter()
            .map(|t| {
                let schema = match t.schema {
                    Value::Object(map) => map,
                    _ => serde_json::Map::new(),
                };
                Tool::new(t.name, t.description, Arc::new(schema))
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(&self, request: CallToolRequestParams, _context: RequestContext<RoleServer>) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        if !tools::catalog().iter().any(|t| t.name == name) {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!("unknown tool {name}"))]).into());
        }
        let args = request.arguments.map(Value::Object).unwrap_or(Value::Object(Default::default()));
        let backend = self.backend.clone();
        let agent = self.agent.clone();
        let result = tokio::task::spawn_blocking(move || backend.call(&name, args, &agent))
            .await
            .map_err(|e| ErrorData::internal_error(format!("tool task failed: {e}"), None))?;
        Ok(match result {
            Ok(value) => CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(&value).unwrap_or_default())]),
            Err(error) => {
                CallToolResult::error(vec![ContentBlock::text(serde_json::to_string_pretty(&error.to_json()).unwrap_or_default())])
            }
        }
        .into())
    }
}

struct Options {
    connect: Option<String>,
    project: Option<String>,
    demo: bool,
    headless: bool,
    agent: String,
}

fn parse_args() -> Result<Options, String> {
    let mut o = Options { connect: None, project: None, demo: false, headless: false, agent: "mcp-agent".into() };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--connect" => o.connect = Some(args.next().ok_or("--connect needs host:port")?),
            "--project" => o.project = Some(args.next().ok_or("--project needs a path")?),
            "--agent" => o.agent = args.next().ok_or("--agent needs a name")?,
            "--demo" => o.demo = true,
            "--headless" => o.headless = true,
            "-h" | "--help" => {
                eprintln!("usage: hephaestus-mcp [--connect HOST:PORT] [--project PATH | --demo | --headless] [--agent NAME]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(o)
}

fn backend(o: &Options) -> Result<(Backend, String), String> {
    let local = o.headless || o.demo || o.project.is_some();
    if !local {
        let addr = o.connect.clone().unwrap_or_else(|| format!("127.0.0.1:{DEFAULT_PORT}"));
        match Client::connect(&addr, &o.agent) {
            Ok(client) => return Ok((Backend::Remote(Mutex::new(client)), format!("connected to Hephaestus at {addr}"))),
            Err(e) if o.connect.is_some() => return Err(format!("cannot connect to Hephaestus at {addr}: {e}")),
            Err(_) => eprintln!("hephaestus-mcp: Hephaestus is not running on {addr}; using a headless session"),
        }
    }
    let mut session = if o.demo { hephaestus_agent_api::demo::flasher().map_err(|e| e.to_string())? } else { Session::new("Untitled") };
    if let Some(path) = &o.project {
        session.open(std::path::Path::new(path)).map_err(|e| e.to_string())?;
    }
    Ok((Backend::Local(Box::new(Mutex::new(session))), "headless session".into()))
}

#[tokio::main]
async fn main() {
    let options = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("hephaestus-mcp: {e}");
            std::process::exit(2);
        }
    };
    let (backend, mode) = match backend(&options) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("hephaestus-mcp: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("hephaestus-mcp: {mode}");
    let server = HephaestusMcp { backend: Arc::new(backend), agent: options.agent, mode };
    match server.serve(rmcp::transport::stdio()).await {
        Ok(service) => {
            if let Err(e) = service.waiting().await {
                eprintln!("hephaestus-mcp: {e}");
            }
        }
        Err(e) => {
            eprintln!("hephaestus-mcp: failed to start: {e}");
            std::process::exit(1);
        }
    }
}
