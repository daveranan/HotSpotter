use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use serde_json::{json, Value};
use tauri::{Emitter, AppHandle, State};

pub const MCP_PORT: u16 = 39871;

pub struct McpBridge {
    pending: Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct McpRequestEvent {
    request_id: String,
    name: String,
    arguments: Value,
}

pub fn start(app: AppHandle) -> McpBridge {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let thread_pending = Arc::clone(&pending);
    thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", MCP_PORT)) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(?error, port = MCP_PORT, "MCP loopback bridge unavailable");
                return;
            }
        };
        tracing::info!(port = MCP_PORT, "MCP loopback bridge ready");
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            let pending = Arc::clone(&thread_pending);
            thread::spawn(move || serve_connection(stream, app, pending));
        }
    });
    McpBridge { pending }
}

fn serve_connection(mut stream: TcpStream, app: AppHandle, pending: Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>) {
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let reader = BufReader::new(reader_stream);
    for line in reader.lines().map_while(Result::ok) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        if request.get("method") != Some(&Value::String("call".into())) { continue; }
        let request_id = request.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
        let name = request.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
        let arguments = request.get("arguments").cloned().unwrap_or_else(|| json!({}));
        if request_id.is_empty() || name.is_empty() { continue; }
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut pending) = pending.lock() { pending.insert(request_id.clone(), sender); }
        let _ = app.emit("mcp-request", McpRequestEvent { request_id: request_id.clone(), name, arguments });
        let response = receiver.recv_timeout(std::time::Duration::from_secs(120)).unwrap_or_else(|_| json!({"isError": true, "error": "Hot Trimmer did not answer the MCP request in time."}));
        if let Ok(mut pending) = pending.lock() { pending.remove(&request_id); }
        let _ = writeln!(stream, "{}", response);
        let _ = stream.flush();
    }
}

#[tauri::command]
pub fn mcp_respond(request_id: String, response: Value, bridge: State<'_, McpBridge>) -> Result<(), String> {
    let sender = bridge.pending.lock().map_err(|_| "MCP bridge state is unavailable")?.remove(&request_id);
    sender.ok_or_else(|| "MCP request is no longer pending".to_owned())?.send(response).map_err(|_| "MCP client disconnected".to_owned())
}
