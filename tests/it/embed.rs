use rhebok::config::EmbedConfig;
use rhebok::embed::{Embedder, OllamaEmbedder};
use std::io::{Read, Write};

fn cfg() -> EmbedConfig {
    toml::from_str("model = \"qwen3-embedding:0.6b\"\ndimensions = 512").unwrap()
}

fn stub(status: &'static str, body: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    url
}

fn query_error(status: &'static str) -> String {
    let mut c = cfg();
    c.ollama_url = stub(status, r#"{"error":"nope"}"#);
    format!(
        "{:#}",
        OllamaEmbedder::new(&c).embed_query("q").unwrap_err()
    )
}

#[test]
fn model_id_includes_dimensions_so_a_dim_change_invalidates_the_index() {
    let e = OllamaEmbedder::new(&cfg());
    // `{model}@{dims}+{scheme}` — dimensions catch incompatible truncations,
    // the scheme catches a change in what text actually gets embedded.
    assert_eq!(e.model_id(), "qwen3-embedding:0.6b@512+qwen3");
}

#[test]
fn model_id_excludes_the_host_so_cosmetic_url_changes_dont_force_reindexing() {
    let cfg: EmbedConfig = toml::from_str(
        "model = \"qwen3-embedding:0.6b\"\ndimensions = 512\nollama_url = \"http://otherhost:11434\"",
    )
    .unwrap();
    let e = OllamaEmbedder::new(&cfg);
    // Different host, same model_id: editing the URL doesn't invalidate the index.
    assert_eq!(e.model_id(), "qwen3-embedding:0.6b@512+qwen3");
}

#[test]
fn model_id_is_independent_of_url_normalization() {
    // Since model_id excludes the host, any config differences in the URL
    // (trailing slash, hostname, port) produce the same model_id.
    let with_slash: EmbedConfig = toml::from_str(
        "model = \"qwen3-embedding:0.6b\"\ndimensions = 512\nollama_url = \"http://localhost:11434/\"",
    )
    .unwrap();
    let without_slash: EmbedConfig = toml::from_str(
        "model = \"qwen3-embedding:0.6b\"\ndimensions = 512\nollama_url = \"http://localhost:11434\"",
    )
    .unwrap();
    assert_eq!(
        OllamaEmbedder::new(&with_slash).model_id(),
        OllamaEmbedder::new(&without_slash).model_id(),
    );
}

#[test]
fn each_model_family_gets_the_prefix_scheme_it_was_trained_on() {
    use rhebok::embed::ollama::PrefixScheme;

    // The default model is Qwen3, which was NOT trained on Nomic's
    // `search_document:` / `search_query:` markers. Applying them anyway put
    // off-distribution text in front of every vector in the index — and the
    // 0.70 threshold was then calibrated on top of that.
    let qwen = PrefixScheme::for_model("qwen3-embedding:0.6b");
    assert_eq!(qwen, PrefixScheme::Qwen3Instruct);
    assert_eq!(qwen.doc("hi"), "hi", "Qwen3 takes the passage bare");
    assert!(
        qwen.query("hi").starts_with("Instruct:"),
        "Qwen3 wraps the query in an instruction, got {:?}",
        qwen.query("hi")
    );

    let nomic = PrefixScheme::for_model("nomic-embed-text");
    assert_eq!(nomic.doc("hi"), "search_document: hi");
    assert_eq!(nomic.query("hi"), "search_query: hi");

    let e5 = PrefixScheme::for_model("multilingual-e5-large");
    assert_eq!(e5.doc("hi"), "passage: hi");
    assert_eq!(e5.query("hi"), "query: hi");

    // An unknown model gets no prefixes. Guessing wrong is worse than not
    // guessing: a wrong scheme silently degrades every vector.
    let unknown = PrefixScheme::for_model("some-new-embedder:v9");
    assert_eq!(unknown, PrefixScheme::None);
    assert_eq!(unknown.doc("hi"), "hi");
    assert_eq!(unknown.query("hi"), "hi");
}

#[test]
fn changing_the_prefix_scheme_invalidates_the_index() {
    // Vectors written under one scheme are not comparable with vectors written
    // under another, so the scheme has to reach `model_id` — that is the value
    // stamped into the index and checked on open. Without it, switching models
    // between two families that happen to share a width would silently mix two
    // embedding spaces.
    let base: EmbedConfig =
        toml::from_str("model = \"qwen3-embedding:0.6b\"\ndimensions = 512").unwrap();
    let other: EmbedConfig =
        toml::from_str("model = \"nomic-embed-text\"\ndimensions = 512").unwrap();

    let a = OllamaEmbedder::new(&base).model_id();
    let b = OllamaEmbedder::new(&other).model_id();
    assert_ne!(a, b);
    assert!(a.contains("qwen3"), "scheme must appear in the id, got {a}");
    assert!(b.contains("nomic"), "scheme must appear in the id, got {b}");
}

#[test]
fn vectors_are_l2_normalized_for_cosine_similarity() {
    let v = rhebok::embed::normalize(vec![3.0, 4.0]);
    assert!((v[0] - 0.6).abs() < 1e-6);
    assert!((v[1] - 0.8).abs() < 1e-6);
}

#[test]
fn normalize_leaves_a_zero_vector_alone_rather_than_producing_nan() {
    let v = rhebok::embed::normalize(vec![0.0, 0.0]);
    assert_eq!(v, vec![0.0, 0.0]);
}

#[test]
fn truncation_to_matryoshka_dimensions_then_renormalizes() {
    let v = rhebok::embed::fit_dimensions(vec![1.0, 1.0, 1.0, 1.0], 2);
    assert_eq!(v.len(), 2);
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
}

#[test]
fn a_zero_vector_stays_zero_and_never_becomes_nan() {
    // This is the shape a zero-padded placeholder would take. `normalize` must
    // pass it through rather than divide by zero — which is exactly why an empty
    // embedding has to be rejected upstream instead of padded.
    let z = rhebok::embed::normalize(vec![0.0; 4]);
    assert!(z.iter().all(|x| *x == 0.0));
    assert!(z.iter().all(|x| !x.is_nan()));
}

#[test]
#[ignore = "requires a running ollama with the model pulled"]
fn live_embedding_returns_configured_dimensions() {
    let e = OllamaEmbedder::new(&cfg());
    let v = e.embed_query("hybrid search").unwrap();
    assert_eq!(v.len(), 512);
}

#[test]
#[ignore = "requires a running ollama with the model pulled"]
fn the_built_in_pairs_put_qwen3_back_on_its_own_gates() {
    let e = rhebok::embed::calibrate::measure(&OllamaEmbedder::new(&cfg())).unwrap();
    let qwen3 = rhebok::embed::choice::QWEN3;
    assert_eq!(
        (e.hook, e.mcp),
        (qwen3.hook_threshold, qwen3.mcp_threshold),
        "{e:?}"
    );
    assert_eq!(e.matched_below_hook, 0, "{e:?}");
}

#[test]
fn a_busy_ollama_is_reported_as_slow_not_as_stopped() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let held: Vec<_> = listener.incoming().collect();
        drop(held);
    });
    let mut c = cfg();
    c.ollama_url = url;
    let err = format!(
        "{:#}",
        OllamaEmbedder::new(&c).embed_query("q").unwrap_err()
    );
    assert!(err.contains("did not answer in time"), "{err}");
    assert!(!err.contains("ollama serve"), "{err}");
}

#[test]
fn a_stopped_ollama_still_says_to_start_it() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut c = cfg();
    c.ollama_url = url;
    let err = format!(
        "{:#}",
        OllamaEmbedder::new(&c).embed_query("q").unwrap_err()
    );
    assert!(err.contains("ollama serve"), "{err}");
}

#[test]
fn an_embedding_outage_names_its_cause_once() {
    let inner = anyhow::Error::new(std::io::Error::other("operation timed out"))
        .context("http://host:1234 did not answer in time");
    let err = format!(
        "{:#}",
        anyhow::Error::new(rhebok::retrieve::EmbedUnavailable(inner))
    );
    assert_eq!(err.matches("did not answer in time").count(), 1, "{err}");
    assert_eq!(err.matches("operation timed out").count(), 1, "{err}");
}

#[test]
fn a_rejected_or_missing_model_is_told_to_pull() {
    for status in ["400 Bad Request", "404 Not Found"] {
        let err = query_error(status);
        assert!(
            err.contains("ollama pull qwen3-embedding:0.6b"),
            "{status}: {err}"
        );
        assert!(err.contains(&status[..3]), "{status}: {err}");
    }
}

#[test]
fn an_ollama_server_error_is_not_blamed_on_the_model() {
    for status in ["500 Internal Server Error", "503 Service Unavailable"] {
        let err = query_error(status);
        assert!(err.contains(&status[..3]), "{status}: {err}");
        assert!(!err.contains("ollama pull"), "{status}: {err}");
        assert!(!err.contains("rejected model"), "{status}: {err}");
    }
}
