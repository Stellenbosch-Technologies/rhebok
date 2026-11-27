use std::collections::BTreeSet;
use std::path::Path;

fn read(relative: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|e| panic!("reading {relative}: {e}"))
}

fn served_routes() -> BTreeSet<(String, String)> {
    let source = read("src/dashboard/http.rs");
    let (post_part, get_part) = source
        .split_once("if method != \"GET\" {")
        .expect("http.rs no longer splits POST routes from GET routes this way");
    let routes_in = |part: &str, method: &str| -> Vec<(String, String)> {
        part.split("\"/api/")
            .skip(1)
            .filter_map(|after| after.split_once('"'))
            .map(|(rest, _)| rest)
            .filter(|rest| !rest.is_empty())
            .map(|rest| (method.to_string(), format!("/api/{rest}")))
            .collect()
    };
    routes_in(post_part, "post")
        .into_iter()
        .chain(routes_in(get_part, "get"))
        .collect()
}

fn documented_routes(spec: &str) -> BTreeSet<(String, String)> {
    let mut routes = BTreeSet::new();
    let mut current_path: Option<String> = None;
    for line in spec.lines() {
        if let Some(path) = line
            .strip_prefix("  /")
            .and_then(|rest| rest.strip_suffix(':'))
        {
            current_path = Some(format!("/{path}"));
        } else if !line.starts_with("  ") && !line.is_empty() {
            current_path = None;
        } else if let (Some(path), Some(method)) = (
            &current_path,
            line.strip_prefix("    ")
                .and_then(|rest| rest.strip_suffix(':')),
        ) {
            if ["get", "post", "put", "patch", "delete"].contains(&method) {
                routes.insert((method.to_string(), path.clone()));
            }
        }
    }
    routes
}

#[test]
fn the_api_reference_documents_exactly_the_routes_the_dashboard_serves() {
    let served = served_routes();
    let documented = documented_routes(&read("fern/openapi/dashboard.yml"));
    assert!(served.len() > 20, "found only {served:?} in http.rs");
    let undocumented: Vec<_> = served.difference(&documented).collect();
    let unserved: Vec<_> = documented.difference(&served).collect();
    assert!(
        undocumented.is_empty() && unserved.is_empty(),
        "fern/openapi/dashboard.yml is out of step with src/dashboard/http.rs\n\
         served but not documented: {undocumented:?}\n\
         documented but not served: {unserved:?}"
    );
}

#[test]
fn the_api_reference_carries_the_crate_version() {
    let spec = read("fern/openapi/dashboard.yml");
    let version = spec
        .lines()
        .find_map(|line| line.strip_prefix("  version: "))
        .expect("fern/openapi/dashboard.yml has no info.version");
    assert_eq!(
        version.trim(),
        env!("CARGO_PKG_VERSION"),
        "bump info.version in fern/openapi/dashboard.yml with Cargo.toml"
    );
}
