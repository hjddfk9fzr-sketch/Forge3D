use super::{rpc_error, Session};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::Read,
    net::SocketAddr,
    path::PathBuf,
    time::{Duration, Instant},
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const MAX_BODY: u64 = 2 * 1024 * 1024;

struct Connection {
    session: Session,
    last_used: Instant,
    protocol: String,
}

pub fn serve(root: PathBuf, address: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("FORGE3D_MCP_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    if !address.ip().is_loopback() && token.is_none() {
        return Err("Set FORGE3D_MCP_TOKEN to listen beyond loopback.".into());
    }
    let origin = std::env::var("FORGE3D_MCP_ORIGIN").ok();
    let server = Server::http(address).map_err(|error| std::io::Error::other(error.to_string()))?;
    eprintln!("Forge3D MCP listening on {}/mcp", server.server_addr());
    let mut clients = HashMap::<String, Connection>::new();
    for mut request in server.incoming_requests() {
        let (status, body, session) = handle(
            &mut request,
            &root,
            &mut clients,
            token.as_deref(),
            origin.as_deref(),
        );
        let mut response = Response::from_data(body).with_status_code(StatusCode(status));
        if status == 200 || status == 400 && response.data_length().unwrap_or(0) > 0 {
            response = response.with_header(header("Content-Type", "application/json"));
        }
        if let Some(id) = session {
            response = response.with_header(header("Mcp-Session-Id", &id));
        }
        if status == 401 {
            response = response.with_header(header("WWW-Authenticate", "Bearer"));
        }
        if status == 405 {
            response = response.with_header(header("Allow", "POST, DELETE"));
        }
        if let Err(error) = request.respond(response) {
            eprintln!("MCP HTTP response failed: {error}");
        }
    }
    Ok(())
}

type Reply = (u16, Vec<u8>, Option<String>);

fn handle(
    request: &mut Request,
    root: &std::path::Path,
    clients: &mut HashMap<String, Connection>,
    token: Option<&str>,
    origin: Option<&str>,
) -> Reply {
    let empty = |status| (status, Vec::new(), None);
    if request.url() != "/mcp" {
        return empty(404);
    }
    if let Some(value) = get(request, "Origin") {
        if Some(value) != origin {
            return empty(403);
        }
    }
    if let Some(token) = token {
        if get(request, "Authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_none_or(|provided| !equal_token(provided, token))
        {
            return empty(401);
        }
    }
    if get(request, "MCP-Protocol-Version").is_some_and(|value| !super::supported(value)) {
        return empty(400);
    }
    let id = get(request, "Mcp-Session-Id").map(str::to_owned);
    clients.retain(|_, client| client.last_used.elapsed() < Duration::from_secs(3600));
    match request.method() {
        Method::Delete => {
            return match id {
                Some(id) if clients.remove(&id).is_some() => empty(204),
                Some(_) => empty(404),
                None => empty(400),
            };
        }
        Method::Post => {}
        _ => return empty(405),
    }
    if get(request, "Content-Type").and_then(|value| value.split(';').next())
        != Some("application/json")
    {
        return empty(415);
    }
    if get(request, "Accept")
        .is_some_and(|value| !value.contains("application/json") && !value.contains("*/*"))
    {
        return empty(406);
    }
    let mut body = Vec::new();
    if request
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .is_err()
    {
        return empty(400);
    }
    if body.len() as u64 > MAX_BODY {
        return empty(413);
    }
    let message: Value = match serde_json::from_slice(&body) {
        Ok(message) => message,
        Err(_) => {
            return (
                400,
                rpc_error(Value::Null, -32700, "Parse error")
                    .to_string()
                    .into_bytes(),
                None,
            )
        }
    };
    if message["method"] == "initialize" {
        if id.is_some() || message.get("id").is_none() {
            return empty(400);
        }
        if clients.len() >= 32 {
            return empty(503);
        }
        let mut random = [0_u8; 24];
        if getrandom::fill(&mut random).is_err() {
            return empty(500);
        }
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let key = URL_SAFE_NO_PAD.encode(random);
        let mut session = Session::new(root.to_owned());
        let response = session.message(message);
        if let Some(response) = response {
            if response.get("result").is_some() {
                clients.insert(
                    key.clone(),
                    Connection {
                        session,
                        last_used: Instant::now(),
                        protocol: response["result"]["protocolVersion"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                    },
                );
                return (200, response.to_string().into_bytes(), Some(key));
            }
            return (400, response.to_string().into_bytes(), None);
        }
        return empty(400);
    }
    let Some(id) = id else { return empty(400) };
    let Some(connection) = clients.get_mut(&id) else {
        return empty(404);
    };
    if get(request, "MCP-Protocol-Version").is_some_and(|version| version != connection.protocol) {
        return empty(400);
    }
    connection.last_used = Instant::now();
    match connection.session.message(message) {
        Some(response) => (200, response.to_string().into_bytes(), None),
        None => empty(202),
    }
}

fn get<'a>(request: &'a Request, field: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(field))
        .map(|header| header.value.as_str())
}

fn header(key: &'static str, value: &str) -> Header {
    Header::from_bytes(key, value).expect("generated header is valid ASCII")
}

fn equal_token(provided: &str, token: &str) -> bool {
    provided.len() == token.len()
        && provided
            .bytes()
            .zip(token.bytes())
            .fold(0, |difference, (a, b)| difference | (a ^ b))
            == 0
}
