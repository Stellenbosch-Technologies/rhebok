use super::DashboardError;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Version {
    pub pid: u32,
    pub installed: String,
    pub latest: Option<String>,
    pub url: Option<String>,
    pub checked_at: Option<u64>,
    pub error: Option<String>,
    pub no_release: bool,
    pub update_available: Option<String>,
    pub update: Option<crate::update::Status>,
}

pub fn version(refresh: bool) -> Result<Version, DashboardError> {
    let paths = crate::setup::Paths::from_env();
    let installed = env!("CARGO_PKG_VERSION");
    if refresh {
        let opts = crate::update::UpdateOpts {
            paths: paths.clone(),
            installed: installed.to_string(),
            api: crate::update::release::api_base(),
            token: crate::update::release::token(),
            target: env!("RHEBOK_TARGET").to_string(),
            check_only: true,
        };
        let _ = crate::update::check(&opts);
    }
    let check = crate::update::UpdateCheck::read(&paths.update_json);
    let running = crate::update::Status::read(&paths.update_status)
        .filter(|s| s.done || crate::index::pid_is_alive(s.pid));
    Ok(Version {
        pid: std::process::id(),
        installed: installed.to_string(),
        latest: check.as_ref().and_then(|c| c.latest.clone()),
        url: check.as_ref().and_then(|c| c.url.clone()),
        checked_at: check.as_ref().map(|c| c.checked_at),
        error: check.as_ref().and_then(|c| c.error.clone()),
        no_release: check.as_ref().is_some_and(|c| c.no_release_published()),
        update_available: check.as_ref().and_then(|c| c.available_against(installed)),
        update: running,
    })
}
