use anyhow::{anyhow, Context, Result};

pub const DEFAULT_API: &str = "https://api.github.com/repos/Stellenbosch-Technologies/rhebok";

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub version: semver::Version,
    pub html_url: String,
    pub assets: Vec<Asset>,
}

pub fn api_base() -> String {
    std::env::var("RHEBOK_RELEASE_API")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API.to_string())
        .trim_end_matches('/')
        .to_string()
}

pub const TOKEN_VAR: &str = "RHEBOK_GITHUB_TOKEN";

pub fn token() -> Option<String> {
    std::env::var(TOKEN_VAR)
        .ok()
        .or_else(|| {
            let env = crate::env_file::env_path(&crate::config::Config::config_path());
            crate::env_file::read_lenient(&env).remove(TOKEN_VAR)
        })
        .filter(|s| !s.trim().is_empty())
}

fn optional_token_hint() -> String {
    format!(
        "{TOKEN_VAR} is optional and only raises GitHub's rate limit; set it to any GitHub token, in your shell or in {}",
        crate::env_file::env_path(&crate::config::Config::config_path()).display()
    )
}

pub fn refusal(url: &str, status: reqwest::StatusCode, token: Option<&str>) -> anyhow::Error {
    match (status.as_u16(), token) {
        (401, Some(_)) => anyhow!(
            "GitHub refused the request (GET {url}: HTTP {status}): it did not accept the token in {TOKEN_VAR}; replace it, or unset it, since no token is needed"
        ),
        (403 | 429, Some(_)) => anyhow!(
            "GitHub refused the request (GET {url}: HTTP {status}): the rate limit for the token in {TOKEN_VAR} is spent, or GitHub did not accept that token; try again later, or unset it"
        ),
        (401 | 403 | 429, None) => anyhow!(
            "GitHub refused the request (GET {url}: HTTP {status}), most often because this network has used up GitHub's limit of 60 anonymous requests an hour; try again later. {}",
            optional_token_hint()
        ),
        _ => anyhow!("GET {url}: HTTP {status}"),
    }
}

pub fn parse_release(v: &serde_json::Value) -> Result<Release> {
    let tag = v["tag_name"].as_str().unwrap_or_default();
    let version = semver::Version::parse(tag.trim_start_matches('v'))
        .with_context(|| format!("release tag `{tag}` is not v<major>.<minor>.<patch>"))?;
    let assets = v["assets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Asset {
                        name: x["name"].as_str()?.to_string(),
                        url: x["url"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        version,
        html_url: v["html_url"].as_str().unwrap_or_default().to_string(),
        assets,
    })
}

pub fn client(agent: &str, timeout: std::time::Duration) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(agent.to_string())
        .build()
        .context("build http client")
}

pub fn fetch_latest(api: &str, token: Option<&str>, agent: &str) -> Result<Option<Release>> {
    fetch_release(api, "latest", token, agent)
}

pub fn fetch_tag(
    api: &str,
    tag: &str,
    token: Option<&str>,
    agent: &str,
) -> Result<Option<Release>> {
    fetch_release(api, &format!("tags/{tag}"), token, agent)
}

fn fetch_release(
    api: &str,
    route: &str,
    token: Option<&str>,
    agent: &str,
) -> Result<Option<Release>> {
    let url = format!("{}/releases/{route}", api.trim_end_matches('/'));
    let mut req = client(agent, std::time::Duration::from_secs(5))?
        .get(&url)
        .header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(refusal(&url, status, token));
    }
    let v: serde_json::Value = resp
        .json()
        .with_context(|| format!("GET {url}: not JSON"))?;
    parse_release(&v).map(Some)
}

pub fn asset<'a>(r: &'a Release, name: &str) -> Result<&'a Asset> {
    r.assets.iter().find(|a| a.name == name).ok_or_else(|| {
        anyhow!(
            "release v{} has no asset named {name} (it has: {})",
            r.version,
            r.assets
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}
