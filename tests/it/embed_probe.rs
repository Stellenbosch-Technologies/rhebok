use rhebok::embed::probe::{endpoint_dimensions, endpoint_models, ollama_dimensions};
use std::io::{Read, Write};

fn stub(status: &'static str, body: String) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 65536];
            let n = stream.read(&mut buf).unwrap_or(0);
            let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (format!("http://{addr}"), rx)
}

fn vector(n: usize) -> String {
    format!("[{}]", vec!["0.1"; n].join(","))
}

#[test]
fn an_ollama_model_reports_its_native_width() {
    let (url, rx) = stub("200 OK", format!(r#"{{"embeddings":[{}]}}"#, vector(768)));
    assert_eq!(ollama_dimensions(&url, "nomic-embed-text").unwrap(), 768);
    let req = rx.recv().unwrap();
    assert!(req.contains("POST /api/embed"), "{req}");
    assert!(req.contains("\"model\":\"nomic-embed-text\""), "{req}");
}

#[test]
fn a_model_that_cannot_embed_is_named_as_probably_a_chat_model() {
    let (url, _rx) = stub(
        "400 Bad Request",
        r#"{"error":"\"llama3\" does not support embeddings"}"#.to_string(),
    );
    let err = ollama_dimensions(&url, "llama3").unwrap_err().to_string();
    assert!(err.contains("does not support embeddings"), "{err}");
    assert!(err.contains("chat model"), "{err}");
}

#[test]
fn an_endpoint_model_reports_its_native_width_and_sends_the_token() {
    let (url, rx) = stub(
        "200 OK",
        format!(r#"{{"data":[{{"index":0,"embedding":{}}}]}}"#, vector(1024)),
    );
    assert_eq!(
        endpoint_dimensions(&url, "bge-m3", Some("tok")).unwrap(),
        1024
    );
    let req = rx.recv().unwrap();
    assert!(req.contains("POST /v1/embeddings"), "{req}");
    assert!(
        req.lines()
            .any(|l| l.eq_ignore_ascii_case("authorization: Bearer tok")),
        "{req}"
    );
}

#[test]
fn an_endpoint_that_wants_a_token_says_so() {
    let (url, _rx) = stub("401 Unauthorized", "{}".to_string());
    let err = endpoint_dimensions(&url, "m", None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("needs a token"), "{err}");
}

#[test]
fn an_endpoints_models_are_listed_without_a_token_when_none_is_given() {
    let (url, rx) = stub(
        "200 OK",
        r#"{"data":[{"id":"text-embedding-bge-m3"},{"id":"qwen3-4b"}]}"#.to_string(),
    );
    assert_eq!(
        endpoint_models(&url, None).unwrap(),
        vec!["text-embedding-bge-m3", "qwen3-4b"]
    );
    let req = rx.recv().unwrap();
    assert!(req.contains("GET /v1/models"), "{req}");
    assert!(
        !req.to_ascii_lowercase().contains("authorization:"),
        "{req}"
    );
}

#[test]
fn an_unreachable_endpoint_is_an_error_not_an_empty_list() {
    assert!(endpoint_models("http://127.0.0.1:1", None).is_err());
}
