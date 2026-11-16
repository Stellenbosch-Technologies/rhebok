use crate::common;
use assert_cmd::Command;
use std::path::{Path, PathBuf};

struct Sandbox {
    t: tempfile::TempDir,
    db: PathBuf,
    cfg: PathBuf,
}

impl Sandbox {
    fn api_dir(&self) -> PathBuf {
        self.t.path().join("work/api")
    }
}

const PROMPT: &str = "why did the connection pooler drop sessions";

fn sandbox(projects: &str, hook_quality: u8) -> Sandbox {
    let t = tempfile::tempdir().unwrap();
    let notes = t.path().join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::create_dir_all(t.path().join("work/api")).unwrap();
    std::fs::write(
        notes.join("api-pool.md"),
        "---\ntags: [api]\n---\n# API pooling\n\nThe api service uses PgBouncer in transaction mode. See [[secret-pool]].",
    )
    .unwrap();
    std::fs::write(
        notes.join("secret-pool.md"),
        "# Secret pooling\n\nThe billing service pooler dropped sessions after the zebracorn migration.",
    )
    .unwrap();
    let ollama = common::fake_ollama();
    let cfg = t.path().join("config.toml");
    let projects = projects.replace("{work}", &t.path().join("work").display().to_string());
    std::fs::write(
        &cfg,
        format!(
            "index_transcripts = false\nsources = [\"{}\"]\n\n[hook]\nquality = {hook_quality}\n\n{projects}\n[embed]\nollama_url = \"{ollama}\"\n",
            notes.display()
        ),
    )
    .unwrap();
    let db = t.path().join("db");
    rhebok(&db, &cfg).arg("index").assert().success();
    Sandbox { t, db, cfg }
}

fn rhebok(db: &Path, cfg: &Path) -> Command {
    let mut c = Command::cargo_bin("rhebok").unwrap();
    c.env("RHEBOK_DB", db).env("RHEBOK_CONFIG", cfg);
    c
}

fn context(s: &Sandbox, prompt: &str, cwd: &Path) -> serde_json::Value {
    let out = rhebok(&s.db, &s.cfg)
        .args(["context", "--json", "--cwd"])
        .arg(cwd)
        .write_stdin(prompt)
        .output()
        .unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}

fn uris(v: &serde_json::Value) -> Vec<String> {
    v["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["uri"].as_str().unwrap().to_string())
        .collect()
}

const API: &str = "[projects.api]\nroots = [\"{work}/api\"]\ninclude = [\"tag:api\"]\n";

#[test]
fn a_project_sees_only_its_own_documents() {
    let s = sandbox(API, 1);
    assert!(s.db.join("pack.scope").exists());
    let inside = context(&s, PROMPT, &s.api_dir());
    assert_eq!(inside["project"], "api");
    let found = uris(&inside);
    assert!(
        found.iter().all(|u| u.ends_with("api-pool.md")),
        "{found:?}"
    );
    assert!(!found.is_empty(), "{inside}");

    let outside = context(&s, PROMPT, s.t.path());
    assert!(outside["project"].is_null());
    assert!(
        uris(&outside).iter().any(|u| u.ends_with("secret-pool.md")),
        "{outside}"
    );
}

#[test]
fn a_keyword_only_answer_outside_the_project_is_never_injected() {
    let s = sandbox(API, 1);
    let v = context(
        &s,
        "what happened during the zebracorn migration",
        &s.api_dir(),
    );
    let block = v["block"].as_str().unwrap_or("");
    assert!(!block.contains("zebracorn"), "{v}");
    assert!(
        uris(&v).iter().all(|u| !u.ends_with("secret-pool.md")),
        "{v}"
    );
}

#[test]
fn graph_expansion_does_not_leak_a_linked_note_from_outside_the_project() {
    let s = sandbox(API, 3);
    let v = context(&s, PROMPT, &s.api_dir());
    assert!(
        uris(&v).iter().all(|u| !u.ends_with("secret-pool.md")),
        "{v}"
    );
    assert!(
        !v["block"].as_str().unwrap_or("").contains("zebracorn"),
        "{v}"
    );
}

#[test]
fn an_edited_rule_refuses_loudly_until_rescoped() {
    let s = sandbox(API, 1);
    let text = std::fs::read_to_string(&s.cfg).unwrap();
    std::fs::write(&s.cfg, text.replace("tag:api", "tag:billing")).unwrap();

    let stale = context(&s, PROMPT, &s.api_dir());
    assert!(stale["block"].is_null(), "{stale}");
    let notes = stale["notes"].to_string();
    assert!(notes.contains("rhebok project rescope"), "{notes}");

    rhebok(&s.db, &s.cfg)
        .args(["project", "rescope"])
        .assert()
        .success();
    let fresh = context(&s, PROMPT, &s.api_dir());
    assert!(fresh["notes"].as_array().unwrap().is_empty(), "{fresh}");
}

#[test]
fn a_no_op_index_refreshes_stale_scopes_by_itself() {
    let s = sandbox(API, 1);
    let text = std::fs::read_to_string(&s.cfg).unwrap();
    std::fs::write(&s.cfg, text.replace("tag:api", "tag:billing")).unwrap();
    rhebok(&s.db, &s.cfg).arg("index").assert().success();
    let v = context(&s, PROMPT, &s.api_dir());
    assert!(v["notes"].as_array().unwrap().is_empty(), "{v}");
}

#[test]
fn explain_names_the_rule_that_decided() {
    let s = sandbox(
        "[projects.api]\nroots = [\"{work}/api\"]\ninclude = [\"tag:api\"]\n\n[projects.api.slices.ops]\ninclude = [\"tag:ops\"]\n",
        1,
    );
    let notes = s.t.path().join("notes");
    let show = |file: &str| {
        let out = rhebok(&s.db, &s.cfg)
            .args(["project", "explain"])
            .arg(notes.join(file))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    let api = show("api-pool.md");
    assert!(api.contains("in   api (included by `tag:api`)"), "{api}");
    assert!(
        api.contains("out  api:ops (no include rule matches)"),
        "{api}"
    );
    let secret = show("secret-pool.md");
    assert!(
        secret.contains("out  api (no include rule matches)"),
        "{secret}"
    );
    assert!(
        secret.contains("out  api:ops (not in project api"),
        "{secret}"
    );
}

#[test]
fn a_large_scope_is_searched_by_the_filtered_index_and_still_stays_inside() {
    use rhebok::pack::records::Record;
    use rhebok::pack::scope::RowSet;
    use rhebok::pack::{Pack, EXACT_SCOPE_ROWS};
    let t = tempfile::tempdir().unwrap();
    let dims = 8;
    let rows = EXACT_SCOPE_ROWS * 2 + 1000;
    let mut data = Vec::new();
    for i in 0..rows {
        let mut v = vec![0.0f32; dims];
        v[i % dims] = 1.0;
        v[(i + 1) % dims] = 0.5;
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter_mut().for_each(|x| *x /= norm);
        let record = Record {
            chunk_id: format!("doc{i:05}:0"),
            doc_id: format!("doc{i:05}"),
            text: format!("row {i}"),
            heading_path: String::new(),
            uri: format!("file:///n/{i}.md"),
            title: format!("{i}"),
            page_no: None,
            source_type: "markdown".into(),
            inbound: 0,
            lifecycle: Default::default(),
            last_used: None,
            memory: None,
        };
        data.push((record, v));
    }
    Pack::build(
        t.path(),
        "fake@8",
        dims,
        data,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let pack = Pack::open(t.path(), "fake@8", dims).unwrap();
    let mut scope = RowSet::empty(rows);
    for row in (0..rows).filter(|r| r % 2 == 0) {
        scope.insert(row);
    }
    assert!(scope.len() > EXACT_SCOPE_ROWS);
    let mut q = vec![0.0f32; dims];
    q[1] = 1.0;
    let hits = pack.search_in(&q, 50, 64, Some(&scope)).unwrap();
    assert_eq!(hits.len(), 50);
    for (record, _) in &hits {
        let row: usize = record.title.parse().unwrap();
        assert!(scope.contains(row), "row {row} is outside the scope");
    }
}

fn bench(s: &Sandbox, golden: &str, args: &[&str]) -> std::process::Output {
    let file = s.t.path().join("golden.toml");
    std::fs::write(&file, golden).unwrap();
    rhebok(&s.db, &s.cfg)
        .env("RHEBOK_GOLDEN", &file)
        .arg("bench")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn a_project_with_no_golden_cases_is_unmeasured_not_passing() {
    let s = sandbox(API, 1);
    let notes = std::fs::canonicalize(s.t.path().join("notes")).unwrap();
    let golden = format!(
        "[[case]]\nquery = \"{PROMPT}\"\nexpect = \"file://{}\"\n",
        notes.join("secret-pool.md").display()
    );
    let out = bench(&s, &golden, &["--project", "api"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("UNMEASURED, not passing"), "{err}");

    let all = bench(&s, &golden, &[]);
    let stdout = String::from_utf8_lossy(&all.stdout);
    assert!(
        stdout.contains("unmeasured projects (no golden case names them): api"),
        "{stdout}"
    );
}

#[test]
fn bench_project_measures_inside_the_scope() {
    let s = sandbox(API, 1);
    let notes = std::fs::canonicalize(s.t.path().join("notes")).unwrap();
    let golden = format!(
        "[[case]]\nquery = \"{PROMPT}\"\nproject = \"api\"\nexpect = \"file://{}\"\n",
        notes.join("api-pool.md").display()
    );
    let out = bench(&s, &golden, &["--project", "api"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("scoped to project `api`: 1 of the index's rows, 1 golden case(s)"),
        "{stdout}"
    );
}
