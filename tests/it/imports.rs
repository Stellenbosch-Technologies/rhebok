use crate::common;
use assert_cmd::Command;
use std::path::PathBuf;

struct Sandbox {
    _t: tempfile::TempDir,
    root: PathBuf,
    db: PathBuf,
    cfg: PathBuf,
}

const PROJECTS: &str = "\
[projects.a]
roots = [\"{root}/work/a\"]
include = [\"tag:a\"]
imports = [\"b:shared\"]

[projects.b]
roots = [\"{root}/work/b\"]
include = [\"tag:b\", \"tag:shared\"]
imports = [\"c:deep\"]

[projects.b.slices.shared]
include = [\"tag:shared\"]

[projects.c]
roots = [\"{root}/work/c\"]
include = [\"tag:deep\"]

[projects.c.slices.deep]
include = [\"tag:deep\"]
";

fn sandbox() -> Sandbox {
    let t = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(t.path()).unwrap();
    let notes = root.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    for p in ["work/a", "work/b", "work/c"] {
        std::fs::create_dir_all(root.join(p)).unwrap();
    }
    let note = |name: &str, tag: &str, body: &str| {
        std::fs::write(
            notes.join(name),
            format!("---\ntags: [{tag}]\n---\n# {name}\n\n{body}"),
        )
        .unwrap();
    };
    note("a.md", "a", "Project a keeps its queue in Redis.");
    note(
        "shared.md",
        "shared",
        "The shared runbook says restart the zebracorn worker first.",
    );
    note(
        "deep.md",
        "deep",
        "The deep note says the zebracorn worker lives on host nine.",
    );
    let ollama = common::fake_ollama();
    let cfg = root.join("config.toml");
    std::fs::write(
        &cfg,
        format!(
            "index_transcripts = false\nsources = [\"{}\"]\n\n{}\n[embed]\nollama_url = \"{ollama}\"\n",
            notes.display(),
            PROJECTS.replace("{root}", &root.display().to_string())
        ),
    )
    .unwrap();
    let s = Sandbox {
        db: root.join("db"),
        cfg,
        root,
        _t: t,
    };
    rhebok(&s).arg("index").assert().success();
    s
}

fn rhebok(s: &Sandbox) -> Command {
    let mut c = Command::cargo_bin("rhebok").unwrap();
    c.env("RHEBOK_DB", &s.db).env("RHEBOK_CONFIG", &s.cfg);
    c
}

fn found_in(s: &Sandbox, project_dir: &str) -> Vec<String> {
    let out = rhebok(s)
        .args(["context", "--json", "--cwd"])
        .arg(s.root.join(project_dir))
        .write_stdin("where does the zebracorn worker run and how do I restart it")
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["notes"].as_array().unwrap().is_empty(), "{v}");
    v["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            h["uri"]
                .as_str()
                .unwrap()
                .rsplit('/')
                .next()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn an_imported_slice_is_searched_but_what_it_imports_is_not() {
    let s = sandbox();
    let from_a = found_in(&s, "work/a");
    assert!(from_a.contains(&"shared.md".to_string()), "{from_a:?}");
    assert!(from_a.contains(&"a.md".to_string()), "{from_a:?}");
    assert!(
        !from_a.contains(&"deep.md".to_string()),
        "a imports b:shared, and b's own import of c:deep must not follow it: {from_a:?}"
    );
    let from_b = found_in(&s, "work/b");
    assert!(from_b.contains(&"deep.md".to_string()), "{from_b:?}");
}

#[test]
fn project_show_lists_each_import_and_what_it_resolves_to() {
    let s = sandbox();
    let out = rhebok(&s)
        .args(["project", "show", "--cwd"])
        .arg(s.root.join("work/a"))
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("imports b:shared: a local slice"), "{text}");
}

#[test]
fn doctor_warns_about_a_project_that_imports_too_many_mounts() {
    let cfg: rhebok::config::Config = toml::from_str(
        "[projects.big]\nroots = [\"/tmp\"]\nimports = [\"m1\", \"m2\", \"m3\"]\n\n[mounts.m1]\npath = \"/x\"\n[mounts.m2]\npath = \"/y\"\n[mounts.m3]\npath = \"/z\"\n",
    )
    .unwrap();
    let warnings = rhebok::project::import_warnings(&cfg);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("imports 3 mounts"), "{warnings:?}");
}
