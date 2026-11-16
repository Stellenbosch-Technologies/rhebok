use assert_cmd::Command;

fn rhebok(t: &std::path::Path) -> Command {
    let notes = t.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("n.md"),
        "# Pooling\n\nPgBouncer drops session state.",
    )
    .unwrap();
    std::fs::write(
        t.join("config.toml"),
        format!(
            "index_transcripts = false\nsources = [\"{}\"]\n\n[embed]\nollama_url = \"http://127.0.0.1:1\"\n",
            notes.display()
        ),
    )
    .unwrap();
    let mut c = Command::cargo_bin("rhebok").unwrap();
    c.env("RHEBOK_DB", t.join("db"))
        .env("RHEBOK_CONFIG", t.join("config.toml"))
        .env("HOME", t)
        .env("RHEBOK_EMBED_URL", "http://127.0.0.1:1")
        .env_remove("RHEBOK_EMBED_MODEL")
        .env("RHEBOK_EMBED_TOKEN", "tok");
    c
}

#[test]
fn an_index_refused_by_a_broken_env_file_leaves_no_shadow_behind() {
    let t = tempfile::tempdir().unwrap();
    let out = rhebok(t.path()).arg("index").output().unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("RHEBOK_EMBED_MODEL"),
        "{out:?}"
    );
    assert!(
        !t.path().join("db.new").exists(),
        "a refused index built a shadow"
    );
}

#[test]
fn status_with_a_broken_env_file_does_not_send_the_user_to_ollama() {
    let t = tempfile::tempdir().unwrap();
    let out = rhebok(t.path()).arg("status").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("MISCONFIGURED"), "{stdout}");
    assert!(!stdout.contains("ollama serve"), "{stdout}");
}
