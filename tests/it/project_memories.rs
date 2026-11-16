use crate::common;
use assert_cmd::Command;
use rhebok::project::{memory_applies, MemoryScope, Origin, Project, ProjectConfig};
use std::path::{Path, PathBuf};

fn project(name: &str, root: &str, memories: MemoryScope) -> Project {
    Project {
        name: name.into(),
        root: PathBuf::from(root),
        origin: Origin::Config,
        rules: ProjectConfig {
            roots: vec![PathBuf::from(root)],
            memories,
            ..Default::default()
        },
    }
}

#[test]
fn which_memories_apply_where() {
    let api = project("api", "/work/api", MemoryScope::Both);
    let here = Some(Path::new("/work/api/src"));
    assert!(
        memory_applies(None, here, Some(&api)),
        "global applies by default"
    );
    assert!(
        memory_applies(Some("/work/api"), here, Some(&api)),
        "saved at the project root"
    );
    assert!(
        memory_applies(Some("/work/api/docs"), here, Some(&api)),
        "saved anywhere inside the project applies across the project"
    );
    assert!(
        memory_applies(Some("api"), here, Some(&api)),
        "saved by project name"
    );
    assert!(
        !memory_applies(Some("/work/web"), here, Some(&api)),
        "another project's memory never applies"
    );

    let only = project("api", "/work/api", MemoryScope::Project);
    assert!(!memory_applies(None, here, Some(&only)));
    assert!(memory_applies(Some("/work/api/docs"), here, Some(&only)));

    let global = project("api", "/work/api", MemoryScope::Global);
    assert!(memory_applies(None, here, Some(&global)));
    assert!(!memory_applies(Some("/work/api"), here, Some(&global)));

    assert!(memory_applies(
        Some("/work/web"),
        Some(Path::new("/work/web/x")),
        None
    ));
    assert!(!memory_applies(
        Some("/work/web"),
        Some(Path::new("/elsewhere")),
        None
    ));
    assert!(
        !memory_applies(Some("/work/web"), None, None),
        "no directory, no local memory"
    );
}

struct Sandbox {
    t: tempfile::TempDir,
    db: PathBuf,
    cfg: PathBuf,
}

fn sandbox(memories: &str) -> Sandbox {
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap();
    let notes = root.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::create_dir_all(root.join("work/api/src")).unwrap();
    std::fs::create_dir_all(root.join("work/web")).unwrap();
    std::fs::write(
        notes.join("n.md"),
        "# Note\n\nUnrelated text about gardening.",
    )
    .unwrap();
    let ollama = common::fake_ollama();
    let cfg = root.join("config.toml");
    std::fs::write(
        &cfg,
        format!(
            "index_transcripts = false\nsources = [\"{}\"]\n\n[projects.api]\nroots = [\"{}\"]\nmemories = \"{memories}\"\n\n[memory]\nduplicate_similarity = 1.5\n\n[embed]\nollama_url = \"{ollama}\"\n",
            notes.display(),
            root.join("work/api").display()
        ),
    )
    .unwrap();
    let db = root.join("db");
    let s = Sandbox { t, db, cfg };
    rhebok(&s).arg("index").assert().success();
    s
}

fn rhebok(s: &Sandbox) -> Command {
    let mut c = Command::cargo_bin("rhebok").unwrap();
    c.env("RHEBOK_DB", &s.db).env("RHEBOK_CONFIG", &s.cfg);
    c
}

fn root(s: &Sandbox) -> PathBuf {
    std::fs::canonicalize(s.t.path()).unwrap()
}

fn remember(s: &Sandbox, text: &str, project: Option<&Path>) {
    let mut c = rhebok(s);
    c.args(["memory", "add", "--kind", "lesson"]);
    match project {
        Some(p) => c.arg("--project").arg(p),
        None => c.arg("--global"),
    };
    c.arg(text).assert().success();
}

fn lessons(s: &Sandbox, cwd: &Path) -> String {
    let out = rhebok(s)
        .args(["hook", "session-start"])
        .write_stdin(serde_json::json!({ "cwd": cwd, "source": "startup" }).to_string())
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn a_lesson_saved_anywhere_in_a_project_reaches_the_whole_project_and_no_other() {
    let s = sandbox("both");
    let r = root(&s);
    remember(
        &s,
        "Always run the api migrations before the api tests.",
        Some(&r.join("work/api")),
    );
    remember(
        &s,
        "Never deploy the web app on a Friday afternoon.",
        Some(&r.join("work/web")),
    );
    remember(&s, "Prefer short plain sentences in every reply.", None);

    let in_api = lessons(&s, &r.join("work/api/src"));
    assert!(in_api.contains("api migrations"), "{in_api}");
    assert!(in_api.contains("plain sentences"), "{in_api}");
    assert!(!in_api.contains("web app"), "{in_api}");
}

#[test]
fn memories_project_keeps_global_lessons_out_of_that_project() {
    let s = sandbox("project");
    let r = root(&s);
    remember(
        &s,
        "Always run the api migrations before the api tests.",
        Some(&r.join("work/api")),
    );
    remember(&s, "Prefer short plain sentences in every reply.", None);
    let in_api = lessons(&s, &r.join("work/api/src"));
    assert!(in_api.contains("api migrations"), "{in_api}");
    assert!(!in_api.contains("plain sentences"), "{in_api}");
}

#[test]
fn another_projects_memory_is_never_retrieved_into_a_prompt() {
    let s = sandbox("both");
    let r = root(&s);
    remember(
        &s,
        "The web app deploy uses the zebracorn pipeline on Fridays.",
        Some(&r.join("work/web")),
    );
    let ask = |cwd: PathBuf| {
        let out = rhebok(&s)
            .args(["context", "--json", "--cwd"])
            .arg(cwd)
            .write_stdin("how does the zebracorn deploy pipeline work")
            .output()
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let from_api = ask(r.join("work/api/src"));
    assert!(
        !from_api["block"]
            .as_str()
            .unwrap_or("")
            .contains("zebracorn"),
        "{from_api}"
    );
    let from_web = ask(r.join("work/web"));
    assert!(
        from_web["block"]
            .as_str()
            .unwrap_or("")
            .contains("zebracorn"),
        "{from_web}"
    );
}

#[test]
fn promote_makes_a_project_memory_global_and_keeps_its_id() {
    let s = sandbox("both");
    let r = root(&s);
    remember(
        &s,
        "The web app deploy uses the zebracorn pipeline on Fridays.",
        Some(&r.join("work/web")),
    );
    let list = || {
        let out = rhebok(&s)
            .args(["memory", "list", "--json"])
            .output()
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let before = list();
    let id = before[0]["id"].as_str().unwrap().to_string();
    assert!(before[0]["facts"]["project"].is_string(), "{before}");

    rhebok(&s)
        .args(["memory", "promote", &id])
        .assert()
        .success();
    let after = list();
    assert_eq!(after[0]["id"], id.as_str());
    assert!(after[0]["facts"]["project"].is_null(), "{after}");
    assert!(lessons(&s, &r.join("work/api/src")).contains("zebracorn"));

    let again = rhebok(&s)
        .args(["memory", "promote", &id])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&again.stdout).contains("already global"));
}
