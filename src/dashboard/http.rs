use super::core::{self, DashboardError};
use super::ConfigSource;
use crate::config::Config;
use crate::setup::agents::AgentEnv;
use anyhow::Result;
use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(rust_embed::RustEmbed)]
#[folder = "dashboard/dist/"]
struct Assets;

fn agent_env(fixed: Option<&AgentEnv>) -> AgentEnv {
    fixed.cloned().unwrap_or_else(AgentEnv::from_env)
}

fn agent_refusal(kind: core::AgentRefusal, message: &str) -> (&'static str, serde_json::Value) {
    let status = match kind {
        core::AgentRefusal::UnknownAgent => "404 Not Found",
        core::AgentRefusal::BadOption => "400 Bad Request",
        core::AgentRefusal::Unavailable | core::AgentRefusal::Unparseable => "409 Conflict",
        core::AgentRefusal::Failed => "500 Internal Server Error",
    };
    (
        status,
        serde_json::json!({ "error": message, "unparseable": kind == core::AgentRefusal::Unparseable }),
    )
}

const MAX_BODY: usize = 64 * 1024;
const DRAIN_LIMIT: usize = MAX_BODY * 2;
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn read_retrying(stream: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    loop {
        match stream.read(buf) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            done => return done,
        }
    }
}

pub(super) fn handle(mut stream: TcpStream, source: &ConfigSource, agents: Option<&AgentEnv>) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut buf = [0u8; 8192];
    let mut req = Vec::new();
    loop {
        match read_retrying(&mut stream, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                req.extend_from_slice(&buf[..n]);
                if req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if req.len() > 64 * 1024 {
                    return;
                }
            }
            Err(_) => return,
        }
    }
    let line = String::from_utf8_lossy(&req);
    let head = line.split("\r\n\r\n").next().unwrap_or("").to_string();
    let mut parts = head.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return;
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if !stream
        .local_addr()
        .map(|l| host_allowed(&head, l))
        .unwrap_or(false)
    {
        return json_status(
            &mut stream,
            "421 Misdirected Request",
            serde_json::json!({ "error": "refused: the Host header does not name this dashboard" }),
        );
    }
    if method == "POST" {
        let header_end = req
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
            .unwrap_or(req.len());
        let content_length: usize = head
            .lines()
            .find_map(|l| {
                l.split_once(':')
                    .filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.trim().parse().ok())
            })
            .unwrap_or(0);
        if content_length > MAX_BODY {
            let mut seen = req.len() - header_end;
            while seen < content_length && seen < DRAIN_LIMIT {
                match read_retrying(&mut stream, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => seen += n,
                    Err(_) => break,
                }
            }
            return json_status(
                &mut stream,
                "413 Payload Too Large",
                serde_json::json!({ "error": "request body too large" }),
            );
        }
        let mut body = req[header_end..].to_vec();
        while body.len() < content_length {
            match read_retrying(&mut stream, &mut buf) {
                Ok(0) => break,
                Ok(n) => body.extend_from_slice(&buf[..n]),
                Err(_) => return,
            }
        }
        body.truncate(content_length);

        let local = stream.local_addr().ok();
        if !local.map(|l| origin_allowed(&head, l)).unwrap_or(false) {
            return json_status(
                &mut stream,
                "409 Conflict",
                serde_json::json!({ "error": "refused: request origin is not this dashboard" }),
            );
        }
        match path {
            "/api/update" => {
                let paths = crate::setup::Paths::from_env();
                let started = running_cli().and_then(|cli| core::update::start(&paths, &cli));
                return accepted(&mut stream, started);
            }
            "/api/restart" => {
                let paths = crate::setup::Paths::from_env();
                return accepted(&mut stream, core::update::restart(&paths));
            }
            "/api/memory/save" => {
                return match serde_json::from_slice::<core::memory_write::SaveRequest>(&body) {
                    Ok(req) => core_result(
                        &mut stream,
                        core::memory_write::save(&source.current(), req),
                    ),
                    Err(_) => body_not_json(&mut stream),
                };
            }
            "/api/agents/connect" => {
                return agent_write(&mut stream, &body, |req| {
                    core::agents::connect(&agent_env(agents), req)
                });
            }
            "/api/agents/disconnect" => {
                return agent_write(&mut stream, &body, |req| {
                    core::agents::disconnect(&agent_env(agents), req)
                });
            }
            "/api/memory/delete" => {
                return match serde_json::from_slice::<core::memory_write::DeleteRequest>(&body) {
                    Ok(req) => core_result(
                        &mut stream,
                        core::memory_write::delete(&source.current(), req),
                    ),
                    Err(_) => body_not_json(&mut stream),
                };
            }
            "/api/config" => {
                return match serde_json::from_slice::<core::config::ConfigSave>(&body) {
                    Ok(req) => core_result(&mut stream, core::config::save(&source.path(), req)),
                    Err(_) => body_not_json(&mut stream),
                };
            }
            "/api/config/check" => {
                return match serde_json::from_slice::<core::config::ConfigCheck>(&body) {
                    Ok(req) => core_result(&mut stream, core::config::check(&source.path(), req)),
                    Err(_) => body_not_json(&mut stream),
                };
            }
            "/api/config/embed" => {
                return match serde_json::from_slice::<core::config::EmbedEndpoint>(&body) {
                    Ok(req) => core_result(
                        &mut stream,
                        core::config::set_embed_endpoint(&source.path(), req),
                    ),
                    Err(_) => body_not_json(&mut stream),
                };
            }
            "/api/index" => {
                let request = if body.iter().all(u8::is_ascii_whitespace) {
                    Ok(IndexBody::default())
                } else {
                    serde_json::from_slice::<IndexBody>(&body)
                };
                return match request {
                    Ok(request) => accepted(
                        &mut stream,
                        running_cli().and_then(|cli| {
                            core::index::start(&Config::db_path(), &cli, request.reindex)
                        }),
                    ),
                    Err(_) => body_not_json(&mut stream),
                };
            }
            "/api/embed/test" => {
                return json_status(
                    &mut stream,
                    "200 OK",
                    ok_json(&core::embed::test(&source.path())),
                );
            }
            _ => {
                return respond(
                    &mut stream,
                    "405 Method Not Allowed",
                    "application/json",
                    br#"{"error":"GET only"}"#,
                );
            }
        }
    }
    if method != "GET" {
        respond(
            &mut stream,
            "405 Method Not Allowed",
            "application/json",
            br#"{"error":"GET only"}"#,
        );
        return;
    }
    match path {
        "/api/stats" => core_result(&mut stream, core::stats::stats(&source.current())),
        "/api/progress" => core_result(&mut stream, core::stats::progress()),
        "/api/version" => core_result(
            &mut stream,
            core::version::version(query.split('&').any(|p| p == "refresh=1")),
        ),
        "/api/graph" => core_result(&mut stream, core::graph::graph(&source.current())),
        "/api/document" => core_result(
            &mut stream,
            core::document::document(&source.current(), &query_param(query, "id")),
        ),
        "/api/search" => core_result(
            &mut stream,
            core::search::search(
                &source.current(),
                &core::search::SearchQuery {
                    q: query_param(query, "q"),
                    tier: raw_tier(query),
                },
            ),
        ),
        "/api/memories" => json_status(
            &mut stream,
            "200 OK",
            ok_json(&core::memories::list(&source.current())),
        ),
        "/api/memory/reach" => core_result(
            &mut stream,
            core::memories::reach(&source.current(), &query_param(query, "id")),
        ),
        "/api/ignore/preview" => core_result(
            &mut stream,
            core::ignore::preview(&source.current(), &first_param(query, "pattern")),
        ),
        "/api/config" => core_result(&mut stream, core::config::view(&source.path())),
        "/api/index" => json_status(
            &mut stream,
            "200 OK",
            ok_json(&core::index::run_status(&Config::db_path())),
        ),
        "/api/install" => core_result(
            &mut stream,
            running_cli().map(|cli| core::install::view(&source.path(), &agent_env(agents), &cli)),
        ),
        "/api/agents" => json_status(
            &mut stream,
            "200 OK",
            ok_json(&core::agents::list(&agent_env(agents))),
        ),
        p if p.starts_with("/api/") => respond(
            &mut stream,
            "404 Not Found",
            "application/json",
            br#"{"error":"unknown endpoint"}"#,
        ),
        _ => {
            let _ = query;
            // The SPA owns routing: any non-API path gets the shell.
            let asset = path.trim_start_matches('/');
            let (file, ctype) = match Assets::get(asset) {
                Some(file) => (file, content_type_for(asset)),
                None => (
                    Assets::get("index.html").expect("index.html is embedded"),
                    "text/html",
                ),
            };
            respond(&mut stream, "200 OK", ctype, &file.data);
        }
    }
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "text/html",
    }
}

fn core_result<T: serde::Serialize>(stream: &mut TcpStream, r: Result<T, DashboardError>) {
    match r {
        Ok(v) => json_status(stream, "200 OK", ok_json(&v)),
        Err(e) => {
            let (status, body) = error_response(&e);
            json_status(stream, status, body);
        }
    }
}

fn agent_write(
    stream: &mut TcpStream,
    body: &[u8],
    act: impl FnOnce(core::agents::AgentRequest) -> Result<core::agents::AgentChanged, DashboardError>,
) {
    match serde_json::from_slice::<core::agents::AgentRequest>(body) {
        Ok(req) => core_result(stream, act(req)),
        Err(e) => core_result::<()>(
            stream,
            Err(DashboardError::BadRequest(format!("bad request body: {e}"))),
        ),
    }
}

#[derive(serde::Deserialize, Default)]
struct IndexBody {
    #[serde(default)]
    reindex: bool,
}

fn running_cli() -> Result<std::path::PathBuf, DashboardError> {
    std::env::current_exe().map_err(|e| DashboardError::Internal(e.to_string()))
}

fn accepted<T: serde::Serialize>(stream: &mut TcpStream, r: Result<T, DashboardError>) {
    match r {
        Ok(v) => json_status(stream, "202 Accepted", ok_json(&v)),
        Err(e) => {
            let (status, body) = error_response(&e);
            json_status(stream, status, body)
        }
    }
}

fn body_not_json(stream: &mut TcpStream) {
    core_result::<()>(
        stream,
        Err(DashboardError::BadRequest(
            "body is not the expected JSON".to_string(),
        )),
    )
}

fn query_param(query: &str, name: &str) -> String {
    let mut found = String::new();
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == name {
            found = url_decode(v);
        }
    }
    found
}

fn first_param(query: &str, name: &str) -> String {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| url_decode(v))
        .unwrap_or_default()
}

fn raw_tier(query: &str) -> Option<u8> {
    let mut tier = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == "tier" {
            tier = v.parse().ok();
        }
    }
    tier
}

fn json_status(stream: &mut TcpStream, status: &str, v: serde_json::Value) {
    respond(stream, status, "application/json", v.to_string().as_bytes());
}

fn host_allowed(head: &str, local: std::net::SocketAddr) -> bool {
    let mut hosts = head.lines().skip(1).filter_map(|l| {
        l.split_once(':')
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case("host"))
            .map(|(_, v)| v.trim())
    });
    let (Some(host), None) = (hosts.next(), hosts.next()) else {
        return false;
    };
    let port = local.port();
    let named = |name: &str| {
        host.eq_ignore_ascii_case(&format!("{name}:{port}"))
            || (port == 80 && host.eq_ignore_ascii_case(name))
    };
    ["127.0.0.1", "localhost", "[::1]"].into_iter().any(named)
}

fn origin_allowed(head: &str, local: std::net::SocketAddr) -> bool {
    let origin = head.lines().find_map(|l| {
        l.split_once(':')
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case("origin"))
            .map(|(_, v)| v.trim().to_string())
    });
    match origin {
        None => true,
        Some(o) => {
            let port = local.port();
            o == format!("http://127.0.0.1:{port}") || o == format!("http://localhost:{port}")
        }
    }
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// Percent-decoding for the one query parameter we accept. `+` is a space.
pub(in crate::dashboard) fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            // `i` is always a char boundary here — `b[i] == b'%'` is an ASCII
            // byte value, which cannot occur as a continuation byte of a
            // multi-byte UTF-8 sequence, so this position always starts a
            // character. `i + 1` is therefore also always a boundary (a
            // one-byte char's only boundary is right after it). `i + 3` is
            // NOT guaranteed: the request is read with
            // `String::from_utf8_lossy`, so a literal (non-percent-encoded)
            // multi-byte character can sit immediately after the `%`, e.g.
            // `%€` — 0x25 then a 3-byte UTF-8 sequence — and `i + 3` then
            // lands one byte inside that character. Slicing `s[i+1..i+3]`
            // across a non-boundary panics the connection thread; skipping
            // the escape and treating it as a literal `%`, exactly like the
            // existing not-valid-hex arm below, keeps this a dropped escape
            // rather than a dropped connection.
            b'%' if i + 3 <= b.len() && s.is_char_boundary(i + 3) => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(in crate::dashboard) fn ok_json<T: serde::Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("a dashboard payload serialises")
}

pub(in crate::dashboard) fn error_response(
    e: &DashboardError,
) -> (&'static str, serde_json::Value) {
    let error = e.to_string();
    match e {
        DashboardError::BadRequest(_) => ("400 Bad Request", serde_json::json!({ "error": error })),
        DashboardError::NotFound(_) => ("404 Not Found", serde_json::json!({ "error": error })),
        DashboardError::Conflict(_) => ("409 Conflict", serde_json::json!({ "error": error })),
        DashboardError::ConfigChanged { etag, .. } => (
            "409 Conflict",
            serde_json::json!({ "error": error, "etag": etag }),
        ),
        DashboardError::AgentRefused { kind, .. } => agent_refusal(*kind, &error),
        DashboardError::Unprocessable(errors) => (
            "422 Unprocessable Entity",
            serde_json::json!({ "error": error, "errors": errors }),
        ),
        DashboardError::Unavailable(_) => (
            "503 Service Unavailable",
            serde_json::json!({ "error": error }),
        ),
        DashboardError::VectorExtensionMissing(_) => (
            "500 Internal Server Error",
            serde_json::json!({ "error": error, "stable": true }),
        ),
        DashboardError::CliMissing { fix } => (
            "503 Service Unavailable",
            serde_json::json!({ "error": error, "fix": fix }),
        ),
        DashboardError::Internal(_) => (
            "500 Internal Server Error",
            serde_json::json!({ "error": error }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::core::AgentRefusal;
    use super::*;

    struct InterruptedBeforeEveryChunk {
        chunks: std::collections::VecDeque<&'static [u8]>,
        interrupt: bool,
    }

    impl Read for InterruptedBeforeEveryChunk {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.interrupt = !self.interrupt;
            if self.interrupt {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let Some(chunk) = self.chunks.pop_front() else {
                return Ok(0);
            };
            buf[..chunk.len()].copy_from_slice(chunk);
            Ok(chunk.len())
        }
    }

    #[test]
    fn a_read_interrupted_by_a_signal_is_retried_and_the_request_arrives_whole() {
        let mut stream = InterruptedBeforeEveryChunk {
            chunks: [
                &b"GET /api/progress HTTP/1.1\r\n"[..],
                b"Host: 127.0.0.1\r\n\r\n",
            ]
            .into(),
            interrupt: false,
        };
        let mut buf = [0u8; 64];
        let mut request = Vec::new();
        loop {
            let n = read_retrying(&mut stream, &mut buf).expect("an interrupted read is retried");
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
        }
        assert_eq!(
            request,
            b"GET /api/progress HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        );
    }

    #[test]
    fn serialisation_goes_through_a_value_so_keys_stay_sorted() {
        #[derive(serde::Serialize)]
        struct Sample {
            zeta: u8,
            alpha: Option<u8>,
        }
        let s = Sample {
            zeta: 1,
            alpha: None,
        };
        assert_eq!(ok_json(&s).to_string(), r#"{"alpha":null,"zeta":1}"#);
    }

    #[test]
    fn every_error_variant_keeps_todays_status_line() {
        let status = |e: DashboardError| error_response(&e);
        assert_eq!(
            status(DashboardError::BadRequest("x".into())).0,
            "400 Bad Request"
        );
        assert_eq!(
            status(DashboardError::NotFound("x".into())).0,
            "404 Not Found"
        );
        assert_eq!(
            status(DashboardError::Conflict("x".into())).0,
            "409 Conflict"
        );
        let (line, body) = status(DashboardError::ConfigChanged {
            message: "changed".into(),
            etag: "abc".into(),
        });
        assert_eq!(line, "409 Conflict");
        assert_eq!(
            body,
            serde_json::json!({ "error": "changed", "etag": "abc" })
        );
        let (line, body) = status(DashboardError::Unprocessable(Vec::new()));
        assert_eq!(line, "422 Unprocessable Entity");
        assert_eq!(body["error"], "not saved: ");
        assert_eq!(
            status(DashboardError::Unavailable("busy".into())),
            (
                "503 Service Unavailable",
                serde_json::json!({ "error": "busy" })
            )
        );
        let (line, body) = status(DashboardError::VectorExtensionMissing("gone".into()));
        assert_eq!(line, "500 Internal Server Error");
        assert_eq!(body["stable"], true);
        let (line, body) = status(DashboardError::CliMissing {
            fix: "install it".into(),
        });
        assert_eq!(line, "503 Service Unavailable");
        assert_eq!(body["fix"], "install it");
        for (kind, line, unparseable) in [
            (AgentRefusal::UnknownAgent, "404 Not Found", false),
            (AgentRefusal::BadOption, "400 Bad Request", false),
            (AgentRefusal::Unavailable, "409 Conflict", false),
            (AgentRefusal::Unparseable, "409 Conflict", true),
            (AgentRefusal::Failed, "500 Internal Server Error", false),
        ] {
            let (got, body) = status(DashboardError::AgentRefused {
                kind,
                message: "m".into(),
            });
            assert_eq!(got, line);
            assert_eq!(
                body,
                serde_json::json!({ "error": "m", "unparseable": unparseable })
            );
        }
        let (line, body) = status(DashboardError::Internal("boom".into()));
        assert_eq!(line, "500 Internal Server Error");
        assert_eq!(body, serde_json::json!({ "error": "boom" }));
    }

    #[test]
    fn a_missing_vector_extension_is_reported_as_itself_and_never_as_a_busy_index() {
        let missing = || {
            anyhow::anyhow!("Catalog exception: extension vector has not been installed").context(
                crate::store::VectorExtensionMissing {
                    dir: std::path::PathBuf::from("/home/someone/.lbdb/extension"),
                },
            )
        };
        let message = missing()
            .downcast_ref::<crate::store::VectorExtensionMissing>()
            .unwrap()
            .to_string();
        assert!(
            message.contains("/home/someone/.lbdb/extension"),
            "{message}"
        );

        let out =
            core::search::failure_message(missing().context("open the store for graph expansion"))
                .expect("a missing extension is a stable state, so search answers it with a 200");
        assert_eq!(out, message);

        let (status, body) = error_response(&DashboardError::from(
            missing().context("open the store for /api/stats"),
        ));
        assert_eq!(status, "500 Internal Server Error");
        assert_eq!(body["error"], message);
        assert_eq!(body["stable"], true);

        let (status, body) =
            error_response(&DashboardError::from(anyhow::anyhow!("database is locked")));
        assert_eq!(status, "503 Service Unavailable");
        assert_eq!(body["error"], "database is locked");
        assert!(body.get("stable").is_none(), "{body}");
    }

    /// A `%` immediately followed by a literal (not percent-encoded)
    /// multi-byte UTF-8 character must not panic.
    ///
    /// The request is read with `String::from_utf8_lossy`, so a query target
    /// like `/api/document?id=%€` reaches `url_decode` as `%€` — `%` (one
    /// byte) then `€` (three bytes: 0xE2 0x82 0xAC). `i + 3` then lands one
    /// byte inside `€`, which is not a char boundary, and slicing across it
    /// panicked the connection thread before the char-boundary guard was
    /// added. A bad escape is treated as a literal `%`, exactly like the
    /// existing not-valid-hex case, so the euro sign itself must survive
    /// intact in the output.
    #[test]
    fn url_decode_does_not_panic_on_a_percent_before_a_multibyte_char() {
        assert_eq!(url_decode("%€"), "%€");
        // The panicking byte offset sits one further in when there is a
        // normal percent-escape ahead of it too.
        assert_eq!(url_decode("a%20%€b"), "a %€b");
    }

    #[test]
    fn static_content_types_map_extensions_accurately() {
        assert_eq!(content_type_for("bundle.js"), "text/javascript");
        assert_eq!(content_type_for("style.css"), "text/css");
        assert_eq!(content_type_for("icon.svg"), "image/svg+xml");
        assert_eq!(content_type_for("icon.png"), "image/png");
        assert_eq!(content_type_for("icon.webp"), "image/webp");
        assert_eq!(content_type_for("font.woff2"), "font/woff2");
        assert_eq!(content_type_for("font.woff"), "font/woff");
        assert_eq!(content_type_for("license.txt"), "text/plain; charset=utf-8");
        assert_eq!(content_type_for("index.html"), "text/html");
        assert_eq!(content_type_for("settings"), "text/html");
    }
}
