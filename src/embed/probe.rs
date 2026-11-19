use anyhow::{bail, Context, Result};
use reqwest::blocking::{Client, RequestBuilder};
use std::time::Duration;

const PROBE_TEXT: &str = "dimension probe";

pub fn authorized(request: RequestBuilder, token: Option<&str>) -> RequestBuilder {
    match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

fn client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("build http client")
}

fn width(vector: &serde_json::Value, what: &str) -> Result<usize> {
    let n = vector.as_array().map(Vec::len).unwrap_or(0);
    if n == 0 {
        bail!("{what} returned no embedding; is it an embedding model?");
    }
    Ok(n)
}

pub fn ollama_dimensions(ollama_url: &str, model: &str) -> Result<usize> {
    let url = ollama_url.trim_end_matches('/');
    let resp = client()
        .post(format!("{url}/api/embed"))
        .json(&serde_json::json!({ "model": model, "input": [PROBE_TEXT] }))
        .send()
        .with_context(|| format!("Ollama is unreachable at {url}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().unwrap_or_default();
    if !status.is_success() {
        bail!(
            "Ollama could not embed with `{model}` ({status}): {}. A chat model cannot embed; \
             choose an embedding model",
            body["error"].as_str().unwrap_or("no reason given")
        );
    }
    width(&body["embeddings"][0], &format!("`{model}`"))
}

pub fn endpoint_dimensions(url: &str, model: &str, token: Option<&str>) -> Result<usize> {
    let url = url.trim_end_matches('/');
    let resp = authorized(client().post(format!("{url}/v1/embeddings")), token)
        .json(&serde_json::json!({ "model": model, "input": [PROBE_TEXT] }))
        .send()
        .with_context(|| format!("{url} is unreachable"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        bail!("{url} refused the request ({status}); it needs a token");
    }
    if !status.is_success() {
        bail!("{url} could not embed with `{model}` ({status}); is it an embedding model?");
    }
    let body: serde_json::Value = resp.json().with_context(|| {
        format!("{url} answered /v1/embeddings with something that is not JSON")
    })?;
    width(
        &body["data"][0]["embedding"],
        &format!("`{model}` at {url}"),
    )
}

pub fn endpoint_models(url: &str, token: Option<&str>) -> Result<Vec<String>> {
    let url = url.trim_end_matches('/');
    let resp = authorized(
        Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("build http client")
            .get(format!("{url}/v1/models")),
        token,
    )
    .send()
    .with_context(|| format!("{url} is unreachable"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("{url} answered {status} to /v1/models");
    }
    let body: serde_json::Value = resp
        .json()
        .with_context(|| format!("{url} answered /v1/models with something that is not JSON"))?;
    Ok(body["data"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}
