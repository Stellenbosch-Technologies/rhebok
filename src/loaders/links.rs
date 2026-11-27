use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

static BARE_WEB_ADDRESS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"https?://[^\s<>"'()\[\]{}]+"#).unwrap());

const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?'];

pub fn in_markdown(text: &str, base_uri: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    let mut targets = Vec::new();
    let mut in_code = false;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(Tag::Link { dest_url, .. }) if !in_code => {
                targets.extend(resolve(&dest_url, base_uri));
            }
            Event::Text(prose) if !in_code => {
                targets.extend(BARE_WEB_ADDRESS.find_iter(&prose).filter_map(|m| {
                    resolve(m.as_str().trim_end_matches(TRAILING_PUNCTUATION), base_uri)
                }));
            }
            _ => {}
        }
    }
    unique_in_order(targets)
}

pub fn resolve(target: &str, base_uri: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() || target.starts_with('#') {
        return None;
    }
    let joined = match Url::parse(target) {
        Ok(absolute) => absolute,
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            Url::parse(base_uri).ok()?.join(target).ok()?
        }
        Err(_) => return None,
    };
    match joined.scheme() {
        "http" | "https" => Some(normalize_web(joined)),
        "file" => existing_file(joined),
        _ => None,
    }
}

pub fn normalize(uri: &str) -> String {
    match Url::parse(uri) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => normalize_web(parsed),
        _ => uri.to_string(),
    }
}

pub fn unique_in_order(targets: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    targets
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

fn normalize_web(mut url: Url) -> String {
    url.set_fragment(None);
    let rendered = url.to_string();
    if url.path() != "/" && url.query().is_none() {
        rendered.trim_end_matches('/').to_string()
    } else {
        rendered
    }
}

fn existing_file(mut url: Url) -> Option<String> {
    url.set_fragment(None);
    url.set_query(None);
    let path = url.to_file_path().ok()?;
    let found = path.canonicalize().ok().or_else(|| {
        path.extension()
            .is_none()
            .then(|| path.with_extension("md").canonicalize().ok())
            .flatten()
    })?;
    found
        .is_file()
        .then(|| format!("file://{}", found.display()))
}
