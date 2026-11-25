use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Response {
    status: u16,
    session: Option<String>,
    body: String,
}

fn request(address: &str, method: &str, headers: &[(&str, &str)], body: &str) -> Response {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "{method} /mcp HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").unwrap();
    }
    write!(stream, "\r\n{body}").unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let session = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("Mcp-Session-Id"))
        .map(|(_, value)| value.trim().to_string());
    Response {
        status,
        session,
        body: body.to_string(),
    }
}

#[test]
fn http_transport_authenticates_and_isolates_scene_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let mut server = Process(
        Command::new(env!("CARGO_BIN_EXE_forge3d-mcp"))
            .arg("--root")
            .arg(temp.path())
            .args(["--http", "127.0.0.1:0"])
            .env("FORGE3D_MCP_TOKEN", "local-integration-test")
            .env_remove("FORGE3D_MCP_ORIGIN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut log = BufReader::new(server.0.stderr.take().unwrap());
    let mut listening = String::new();
    log.read_line(&mut listening).unwrap();
    let address = listening
        .trim()
        .strip_prefix("Forge3D MCP listening on ")
        .unwrap()
        .strip_suffix("/mcp")
        .unwrap();
    let authorization = ("Authorization", "Bearer local-integration-test");
    let initialize = json!({"jsonrpc":"2.0","method":"initialize","id":1,"params":{"protocolVersion":"2025-11-25"}}).to_string();

    assert_eq!(request(address, "POST", &[], &initialize).status, 401);
    assert_eq!(
        request(
            address,
            "POST",
            &[authorization, ("Origin", "https://invalid.example")],
            &initialize
        )
        .status,
        403
    );

    let start = || {
        let response = request(address, "POST", &[authorization], &initialize);
        assert_eq!(response.status, 200);
        response.session.unwrap()
    };
    let first = start();
    let second = start();
    assert_ne!(first, second);

    let headers = |id| {
        [
            authorization,
            ("Mcp-Session-Id", id),
            ("MCP-Protocol-Version", "2025-11-25"),
        ]
    };
    let call = |id, name, arguments| {
        let body = json!({"jsonrpc":"2.0","method":"tools/call","id":2,"params":{"name":name,"arguments":arguments}}).to_string();
        let response = request(address, "POST", &headers(id), &body);
        assert_eq!(response.status, 200);
        serde_json::from_str::<Value>(&response.body).unwrap()["result"]["structuredContent"]
            .clone()
    };
    assert_eq!(
        call(&first, "scene_new", json!({"demo":true}))["objects"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        call(&second, "scene_snapshot", json!({}))["objects"],
        json!([])
    );

    let notification = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    assert_eq!(
        request(address, "POST", &headers(&first), notification).status,
        202
    );
    assert_eq!(request(address, "GET", &headers(&first), "").status, 405);
    assert_eq!(request(address, "DELETE", &headers(&first), "").status, 204);
    assert_eq!(
        request(address, "POST", &headers(&first), notification).status,
        404
    );
}
