use rhebok::setup::ollama::{preflight, probe, status_lines, Consent, OllamaModels, OllamaState};
use std::io::{Read, Write};

fn serve(body: &'static str) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    format!("http://{addr}")
}

const TAGS: &str = r#"{"models":[{"name":"qwen3-embedding:0.6b","model":"qwen3-embedding:0.6b"}]}"#;

#[test]
fn a_closed_port_is_unreachable_with_the_reason() {
    match probe("http://127.0.0.1:1", &["x".to_string()]) {
        OllamaState::Unreachable(why) => assert!(!why.is_empty()),
        other => panic!("expected Unreachable, got {other:?}"),
    }
}

#[test]
fn present_and_missing_models_are_told_apart() {
    let url = serve(TAGS);
    match probe(
        &url,
        &["qwen3-embedding:0.6b".to_string(), "qwen3:0.6b".to_string()],
    ) {
        OllamaState::Reachable { missing } => assert_eq!(missing, vec!["qwen3:0.6b".to_string()]),
        other => panic!("{other:?}"),
    }
}

static PULLED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static PULLED_TESTS_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn record_pull(m: &str) -> anyhow::Result<()> {
    PULLED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(m.to_string());
    Ok(())
}
fn refuse(_: &str) -> bool {
    false
}
fn accept(_: &str) -> bool {
    true
}

fn required(models: &[&str]) -> OllamaModels {
    OllamaModels {
        required: models.iter().map(|m| m.to_string()).collect(),
        optional: vec![],
    }
}

fn consent(yes: bool, quiet: bool, confirm: fn(&str) -> bool) -> Consent {
    Consent {
        yes,
        quiet,
        confirm,
    }
}

#[test]
fn a_missing_model_is_pulled_only_with_consent() {
    let _serial = PULLED_TESTS_SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PULLED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    let url = serve(TAGS);
    let models = required(&["qwen3:0.6b"]);
    let (_, warnings) = preflight(&url, &models, &consent(false, false, refuse), record_pull);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("ollama pull qwen3:0.6b")),
        "{warnings:?}"
    );
    assert!(PULLED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_empty());
    let (lines, warnings) = preflight(&url, &models, &consent(false, false, accept), record_pull);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        lines.iter().any(|l| l.contains("pulled qwen3:0.6b")),
        "{lines:?}"
    );
    assert_eq!(
        *PULLED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec!["qwen3:0.6b".to_string()]
    );
}

#[test]
fn yes_skips_the_prompt_and_unreachable_names_ollama_serve() {
    let _serial = PULLED_TESTS_SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let url = serve(TAGS);
    PULLED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    let (_, warnings) = preflight(
        &url,
        &required(&["m"]),
        &consent(true, true, refuse),
        record_pull,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        *PULLED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec!["m".to_string()]
    );
    let (_, warnings) = preflight(
        "http://127.0.0.1:1",
        &required(&["m"]),
        &consent(true, false, accept),
        record_pull,
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("ollama serve") && w.contains("ollama.com")),
        "{warnings:?}"
    );
}

static ASKED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
fn record_question(q: &str) -> bool {
    ASKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(q.to_string());
    false
}

#[test]
fn the_pull_prompt_sizes_each_model_instead_of_one_guess() {
    let url = serve(TAGS);
    let _serial = PULLED_TESTS_SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ASKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    let models = required(&["qwen3:4b-instruct", "someone/custom:7b"]);
    let _ = preflight(
        &url,
        &models,
        &consent(false, false, record_question),
        record_pull,
    );
    let asked = ASKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0].contains("qwen3:4b-instruct (about 2.5 GB)"),
        "{asked:?}"
    );
    assert!(
        asked[0].contains("someone/custom:7b (size unknown)"),
        "{asked:?}"
    );
    assert!(!asked[0].contains("about 1 GB"), "{asked:?}");
}

const DISTILL: &str = "qwen3:4b-instruct";

fn embedder_present_distiller_missing() -> OllamaModels {
    OllamaModels {
        required: vec!["qwen3-embedding:0.6b".to_string()],
        optional: vec![DISTILL.to_string()],
    }
}

fn never_asked(q: &str) -> bool {
    panic!("an unattended run must not ask anything, but asked: {q}");
}

#[test]
fn an_unattended_install_or_update_never_pulls_the_optional_distill_model() {
    let _serial = PULLED_TESTS_SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let url = serve(TAGS);
    for (yes, quiet) in [(true, true), (true, false), (false, true)] {
        PULLED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        let (_, warnings) = preflight(
            &url,
            &embedder_present_distiller_missing(),
            &consent(yes, quiet, never_asked),
            record_pull,
        );
        assert!(
            PULLED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty(),
            "yes={yes} quiet={quiet}: pulled {:?}",
            PULLED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(&format!("ollama pull {DISTILL}"))
                    && w.contains("distill_episodes = false")
                    && w.contains("optional")),
            "yes={yes} quiet={quiet}: {warnings:?}"
        );
    }
}

#[test]
fn an_interactive_yes_pulls_the_optional_distill_model() {
    let _serial = PULLED_TESTS_SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PULLED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    let url = serve(TAGS);
    let (lines, warnings) = preflight(
        &url,
        &embedder_present_distiller_missing(),
        &consent(false, false, accept),
        record_pull,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.contains(&format!("pulled {DISTILL}"))),
        "{lines:?}"
    );
    assert_eq!(
        *PULLED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![DISTILL.to_string()]
    );
}

const NOTHING_PULLED: &str = r#"{"models":[]}"#;

fn yes_unless_optional(q: &str) -> bool {
    ASKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(q.to_string());
    !q.contains("optional")
}

#[test]
fn declining_the_distill_model_still_pulls_the_embedder_it_was_asked_about_separately() {
    let _serial = PULLED_TESTS_SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PULLED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    ASKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    let url = serve(NOTHING_PULLED);
    let models = OllamaModels {
        required: vec!["qwen3-embedding:0.6b".to_string()],
        optional: vec![DISTILL.to_string()],
    };
    let (_, warnings) = preflight(
        &url,
        &models,
        &consent(false, false, yes_unless_optional),
        record_pull,
    );
    let asked = ASKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(
        asked[0].contains("qwen3-embedding:0.6b") && !asked[0].contains(DISTILL),
        "{asked:?}"
    );
    assert!(
        asked[1].contains(DISTILL) && !asked[1].contains("qwen3-embedding"),
        "{asked:?}"
    );
    assert_eq!(
        *PULLED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec!["qwen3-embedding:0.6b".to_string()]
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains(&format!("ollama pull {DISTILL}"))),
        "{warnings:?}"
    );
}

#[test]
fn with_a_remote_embedder_an_unreachable_ollama_is_about_distillation_only() {
    let only_distill = OllamaModels {
        required: vec![],
        optional: vec![DISTILL.to_string()],
    };
    let (_, warnings) = preflight(
        "http://127.0.0.1:1",
        &only_distill,
        &consent(false, false, never_asked),
        record_pull,
    );
    let status = status_lines("http://127.0.0.1:1", &only_distill);
    for said in [&warnings, &status] {
        assert_eq!(said.len(), 1, "{said:?}");
        let line = &said[0];
        assert!(line.contains("optional session distillation"), "{line}");
        assert!(line.contains("Embedding does not need it"), "{line}");
        assert!(line.contains("distill_episodes = false"), "{line}");
        assert!(!line.contains("rhebok install"), "{line}");
        assert!(!line.contains("start it with"), "{line}");
    }

    let local = required(&["qwen3-embedding:0.6b"]);
    let status = status_lines("http://127.0.0.1:1", &local);
    assert!(
        status[0].contains("start it with `ollama serve`"),
        "{status:?}"
    );
}

#[test]
fn status_reports_a_missing_distill_model_as_optional_and_a_missing_embedder_as_missing() {
    let url = serve(NOTHING_PULLED);
    let lines = status_lines(&url, &embedder_present_distiller_missing());
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("reachable, missing model(s): qwen3-embedding:0.6b"),
        "{lines:?}"
    );
    assert!(!lines[0].contains(DISTILL), "{lines:?}");
    assert!(
        lines[1].contains("optional") && lines[1].contains(&format!("ollama pull {DISTILL}")),
        "{lines:?}"
    );
}

fn serve_ollama(version: &'static str) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let body = if request.starts_with("GET /api/version") {
                format!(r#"{{"version":"{version}"}}"#)
            } else {
                TAGS.to_string()
            };
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    format!("http://{addr}")
}

fn pull_must_not_run(m: &str) -> anyhow::Result<()> {
    panic!("pulled {m} on an Ollama too old to run it")
}

#[test]
fn embeddinggemma_is_not_pulled_on_an_ollama_too_old_to_run_it() {
    let url = serve_ollama("0.32.15");
    let models = required(&["embeddinggemma-2:270m-bf16-text"]);
    let (lines, warnings) = preflight(
        &url,
        &models,
        &consent(true, false, accept),
        pull_must_not_run,
    );
    assert!(lines.is_empty(), "{lines:?}");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("needs Ollama 0.40.0 or newer and this one is 0.32.15")),
        "{warnings:?}"
    );
    let status = status_lines(&url, &models);
    assert!(
        status.iter().any(|l| l.contains("needs Ollama 0.40.0")),
        "{status:?}"
    );
    assert!(
        !status.iter().any(|l| l.contains("run: ollama pull")),
        "status must not tell you to pull what this Ollama refuses: {status:?}"
    );
}

#[test]
fn embeddinggemma_is_pulled_on_an_ollama_new_enough_to_run_it() {
    let _serial = PULLED_TESTS_SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PULLED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    let url = serve_ollama("0.40.16");
    let models = required(&["embeddinggemma-2:270m-bf16-text"]);
    let (lines, warnings) = preflight(&url, &models, &consent(true, false, accept), record_pull);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("pulled embeddinggemma-2:270m-bf16-text")),
        "{lines:?}"
    );
}
