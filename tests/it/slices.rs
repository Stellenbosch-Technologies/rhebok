use crate::common;
use assert_cmd::Command;
use std::path::{Path, PathBuf};

struct Machine {
    _t: tempfile::TempDir,
    root: PathBuf,
    db: PathBuf,
    cfg: PathBuf,
}

fn machine(notes: &[(&str, &str)], extra: &str, with_transcript: bool) -> Machine {
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap();
    let dir = root.join("notes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(root.join("work/api")).unwrap();
    for (name, text) in notes {
        std::fs::write(dir.join(name), text).unwrap();
    }
    if with_transcript {
        let projects = root.join("home/.claude/projects/-work-api");
        std::fs::create_dir_all(&projects).unwrap();
        let session = projects.join("11111111-2222-3333-4444-555555555555.jsonl");
        std::fs::copy("tests/fixtures/distill/finished.jsonl", &session).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(5 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&session)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let ollama = common::fake_ollama();
    let cfg = root.join("config.toml");
    let extra = extra.replace("{root}", &root.display().to_string());
    std::fs::write(
        &cfg,
        format!(
            "index_transcripts = {with_transcript}\nsources = [\"{}\"]\n\n{extra}\n[embed]\nollama_url = \"{ollama}\"\n",
            dir.display()
        ),
    )
    .unwrap();
    let m = Machine {
        db: root.join("db"),
        cfg,
        root,
        _t: t,
    };
    rhebok(&m).arg("index").assert().success();
    m
}

fn rhebok(m: &Machine) -> Command {
    let mut c = Command::cargo_bin("rhebok").unwrap();
    c.env("RHEBOK_DB", &m.db)
        .env("RHEBOK_CONFIG", &m.cfg)
        .env("HOME", m.root.join("home"))
        .env("CODEX_HOME", m.root.join("home/.codex"));
    c
}

fn stdout(c: &mut Command) -> String {
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

const TEAM: &str = "[projects.team]\nroots = [\"{root}/work/api\"]\ninclude = [\"tag:adr\", \"agent:claude\"]\n\n[projects.team.slices.adr]\ninclude = [\"tag:adr\", \"agent:claude\"]\n";

fn exporter() -> Machine {
    machine(
        &[
            (
                "adr-pool.md",
                "---\ntags: [adr]\n---\n# ADR pooling\n\nWe chose PgBouncer in transaction mode for the zebracorn service.",
            ),
            ("private.md", "# Private\n\nMy salary review notes, never to be shared."),
        ],
        TEAM,
        true,
    )
}

#[test]
fn a_dry_run_lists_what_would_leave_and_leaves_transcripts_out() {
    let a = exporter();
    let out = stdout(
        rhebok(&a)
            .args(["project", "export", "team:adr", "--dry-run", "--out"])
            .arg(a.root.join("never")),
    );
    assert!(out.contains("adr-pool.md"), "{out}");
    assert!(!out.contains("private.md"), "{out}");
    assert!(!out.contains("claude-session://"), "{out}");
    assert!(out.contains("left out 1 session transcript(s)"), "{out}");
    assert!(!a.root.join("never").exists());
}

fn export_and_mount() -> (Machine, Machine) {
    let a = exporter();
    let slice = a.root.join("slice");
    let out = stdout(
        rhebok(&a)
            .args(["project", "export", "team:adr", "--out"])
            .arg(&slice),
    );
    assert!(out.contains("from 1 document(s)"), "{out}");
    assert!(slice.join("slice.json").is_file());
    let b = machine(
        &[(
            "garden.md",
            "# Garden\n\nTomatoes want full sun and deep watering.",
        )],
        "[projects.api]\nroots = [\"{root}/work/api\"]\nimports = [\"team:adr\"]\n",
        false,
    );
    stdout(rhebok(&b).args(["project", "mount"]).arg(&slice));
    (a, b)
}

fn context(m: &Machine, prompt: &str) -> serde_json::Value {
    let out = stdout(
        rhebok(m)
            .args(["context", "--json", "--cwd"])
            .arg(m.root.join("work/api"))
            .write_stdin(prompt.to_string()),
    );
    serde_json::from_str(&out).unwrap()
}

#[test]
fn a_mounted_slice_answers_inside_the_project_that_imports_it() {
    let (_a, b) = export_and_mount();
    let cfg = std::fs::read_to_string(&b.cfg).unwrap();
    assert!(cfg.contains("[mounts"), "{cfg}");
    let v = context(&b, "which pooler did we choose for the zebracorn service");
    assert!(
        v["block"].as_str().unwrap_or("").contains("zebracorn"),
        "{v}"
    );
    let hit = v["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["uri"].as_str().unwrap().ends_with("adr-pool.md"))
        .expect("the mounted note");
    assert_eq!(hit["mount"], "team:adr");
}

#[test]
fn a_mounted_document_never_gets_a_usage_record() {
    let (a, b) = export_and_mount();
    context(&b, "which pooler did we choose for the zebracorn service");
    let dir = b.db.parent().unwrap();
    let mut logged = String::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with("db.usage.") {
            logged.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
        }
    }
    let exported = rhebok::model::Document::new_id(&format!(
        "file://{}",
        a.root.join("notes/adr-pool.md").display()
    ));
    assert!(!logged.contains(&exported), "{logged}");
    assert!(
        !logged.is_empty(),
        "the machine's own hits are still recorded"
    );
}

#[test]
fn a_slice_from_a_different_embedding_model_is_refused() {
    let a = exporter();
    let slice = a.root.join("slice");
    stdout(
        rhebok(&a)
            .args(["project", "export", "team:adr", "--out"])
            .arg(&slice),
    );
    let b = machine(&[("g.md", "# G\n\ngarden")], "", false);
    let text = std::fs::read_to_string(&b.cfg).unwrap();
    std::fs::write(
        &b.cfg,
        text.replace("[embed]\n", "[embed]\nmodel = \"other-model\"\n"),
    )
    .unwrap();
    let out = rhebok(&b)
        .args(["project", "mount"])
        .arg(&slice)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("same embedding model"), "{err}");
}

#[test]
fn config_check_accepts_an_import_of_a_mount_and_names_a_missing_one() {
    let ok = rhebok::config::check::check(
        "[projects.api]\nroots = [\"/tmp\"]\nimports = [\"team:adr\"]\n\n[mounts.\"team:adr\"]\npath = \"/nowhere\"\n",
    );
    let text: Vec<String> = ok.iter().map(|e| e.to_string()).collect();
    assert!(!text.iter().any(|t| t.contains("imports")), "{text:?}");
    assert!(
        text.iter()
            .any(|t| t.contains("is not an exported rhebok slice")),
        "{text:?}"
    );
}

#[test]
fn export_refuses_a_directory_that_holds_something_else() {
    let a = exporter();
    let busy = a.root.join("busy");
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::write(busy.join("keep.txt"), "mine").unwrap();
    let out = rhebok(&a)
        .args(["project", "export", "team:adr", "--out"])
        .arg(&busy)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("is not empty"));
    assert!(Path::new(&busy.join("keep.txt")).exists());
}
