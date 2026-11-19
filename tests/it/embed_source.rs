use rhebok::embed::choice::{GEMMA, QWEN3};
use rhebok::embed::source::{parse, ModelSource};

fn ollama(m: &str) -> ModelSource {
    ModelSource::Ollama(m.to_string())
}

fn endpoint(u: &str) -> ModelSource {
    ModelSource::Endpoint(u.to_string())
}

#[test]
fn the_presets_are_still_chosen_by_number_name_or_enter() {
    for (input, want) in [
        ("", &QWEN3),
        ("1", &QWEN3),
        ("2", &GEMMA),
        ("qwen3", &QWEN3),
        (" Gemma ", &GEMMA),
    ] {
        assert_eq!(
            parse(input).unwrap(),
            ModelSource::Preset(want),
            "{input:?}"
        );
    }
}

#[test]
fn an_ollama_library_link_becomes_the_model_it_names() {
    for (input, want) in [
        (
            "https://ollama.com/library/nomic-embed-text",
            "nomic-embed-text",
        ),
        (
            "https://ollama.com/library/nomic-embed-text/",
            "nomic-embed-text",
        ),
        (
            "https://www.ollama.com/library/nomic-embed-text",
            "nomic-embed-text",
        ),
        (
            "https://ollama.com/library/nomic-embed-text:v1.5",
            "nomic-embed-text:v1.5",
        ),
        (
            "https://ollama.com/library/embeddinggemma-2",
            "embeddinggemma-2",
        ),
        ("https://ollama.com/library/bge-m3/tags", "bge-m3"),
        ("https://ollama.com/library/bge-m3?tab=readme", "bge-m3"),
        (
            "https://ollama.com/someone/their-embedder",
            "someone/their-embedder",
        ),
    ] {
        assert_eq!(parse(input).unwrap(), ollama(want), "{input}");
    }
}

#[test]
fn a_hugging_face_link_becomes_an_ollama_hf_model() {
    for input in [
        "https://huggingface.co/acme/embedder-GGUF",
        "https://huggingface.co/acme/embedder-GGUF/",
        "https://huggingface.co/acme/embedder-GGUF/tree/main",
        "https://huggingface.co/acme/embedder-GGUF/blob/main/model.Q8_0.gguf",
        "https://hf.co/acme/embedder-GGUF",
    ] {
        assert_eq!(
            parse(input).unwrap(),
            ollama("hf.co/acme/embedder-GGUF"),
            "{input}"
        );
    }
}

#[test]
fn a_bare_name_is_an_ollama_model() {
    assert_eq!(
        parse("nomic-embed-text").unwrap(),
        ollama("nomic-embed-text")
    );
    assert_eq!(
        parse("hf.co/acme/embedder-GGUF:Q8_0").unwrap(),
        ollama("hf.co/acme/embedder-GGUF:Q8_0")
    );
}

#[test]
fn a_link_to_a_presets_exact_model_keeps_the_preset_and_its_gates() {
    assert_eq!(
        parse("https://ollama.com/library/embeddinggemma-2:270m-bf16-text").unwrap(),
        ModelSource::Preset(&GEMMA)
    );
    assert_eq!(
        parse("qwen3-embedding:0.6b").unwrap(),
        ModelSource::Preset(&QWEN3)
    );
}

#[test]
fn any_other_http_uri_is_an_embedding_endpoint() {
    for (input, want) in [
        ("http://localhost:1234", "http://localhost:1234"),
        ("http://localhost:1234/", "http://localhost:1234"),
        ("http://192.168.1.20:1234", "http://192.168.1.20:1234"),
        (
            "https://gpu-box.example:8443/",
            "https://gpu-box.example:8443",
        ),
        ("http://localhost:11434/v1?x=1", "http://localhost:11434/v1"),
    ] {
        assert_eq!(parse(input).unwrap(), endpoint(want), "{input}");
    }
}

#[test]
fn input_that_names_nothing_usable_is_refused_with_an_example() {
    let not_a_model = parse("https://ollama.com/search?q=embed")
        .unwrap_err()
        .to_string();
    assert!(not_a_model.contains("ollama.com/library/"), "{not_a_model}");
    let no_repo = parse("https://huggingface.co/acme")
        .unwrap_err()
        .to_string();
    assert!(no_repo.contains("huggingface.co/"), "{no_repo}");
    assert!(parse("ftp://files.example/model").is_err());
    assert!(parse("two words").is_err());
}
