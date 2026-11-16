use crate::common;

use rhebok::backup::crypto::Key;
use rhebok::backup::manifest::{blob_key, index_key, memory_key, IndexEntry, Manifest};
use rhebok::backup::remote::{FakeRemote, Remote};
use rhebok::backup::{
    back_up, resolve_generation, restore, MemoryOutcome, RestoreOptions, RestoreStats,
};
use rhebok::config::{BackupConfig, Config};
use std::path::PathBuf;

const CONFIG: &str = "sources = []\n";
const GOLDEN: &str = "[[case]]\nquery = \"x\"\n";

struct Fixture {
    _db: common::EnvVarGuard,
    _cfg: common::EnvVarGuard,
    data: PathBuf,
    _home: tempfile::TempDir,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    fn new() -> Fixture {
        let lock = common::lock_env_vars();
        let home = tempfile::tempdir().unwrap();
        let data = home.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("config.toml"), CONFIG).unwrap();
        std::fs::write(data.join("golden.toml"), GOLDEN).unwrap();
        Fixture {
            _cfg: common::EnvVarGuard::set("RHEBOK_CONFIG", data.join("config.toml")),
            _db: common::EnvVarGuard::set("RHEBOK_DB", data.join("db")),
            data,
            _home: home,
            _lock: lock,
        }
    }

    fn config(&self) -> PathBuf {
        self.data.join("config.toml")
    }

    fn golden(&self) -> PathBuf {
        self.data.join("golden.toml")
    }

    fn db(&self) -> PathBuf {
        self.data.join("db")
    }
}

fn files_only() -> Config {
    Config {
        index_transcripts: false,
        backup: BackupConfig {
            include_index: false,
            ..Default::default()
        },
        ..Config::default()
    }
}

fn with_index() -> Config {
    Config {
        index_transcripts: false,
        backup: BackupConfig {
            include_index: true,
            ..Default::default()
        },
        ..Config::default()
    }
}

fn files() -> RestoreOptions {
    RestoreOptions::default()
}

fn manifest_of(remote: &FakeRemote, generation: &str) -> Manifest {
    serde_json::from_slice(&remote.get_bytes(generation).unwrap()).unwrap()
}

#[test]
fn a_deleted_file_is_restored_and_an_existing_one_is_left_alone() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, Some(&key)).unwrap();

    std::fs::remove_file(fx.golden()).unwrap();
    std::fs::write(fx.config(), "sources = [\"edited since the backup\"]\n").unwrap();

    let stats = restore(&files_only(), &remote, Some(&key), &files()).unwrap();
    let written = manifest_of(&remote, &stats.generation);
    assert_eq!(
        stats,
        RestoreStats {
            generation: written.generation_key(),
            created_at: written.created_at,
            host: written.host,
            newer_generations: 0,
            files: 1,
            index_restored: false,
            memory: MemoryOutcome::NotInBackup,
            skipped_existing: 1
        }
    );
    assert_eq!(std::fs::read_to_string(fx.golden()).unwrap(), GOLDEN);
    assert_eq!(
        std::fs::read_to_string(fx.config()).unwrap(),
        "sources = [\"edited since the backup\"]\n",
        "an existing config must not be clobbered without --force"
    );
}

#[test]
fn force_overwrites_existing_files() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, Some(&key)).unwrap();
    std::fs::write(fx.config(), "edited\n").unwrap();

    let opts = RestoreOptions {
        force: true,
        ..files()
    };
    let stats = restore(&files_only(), &remote, Some(&key), &opts).unwrap();
    assert_eq!(stats.files, 2);
    assert_eq!(stats.skipped_existing, 0);
    assert_eq!(std::fs::read_to_string(fx.config()).unwrap(), CONFIG);
}

#[cfg(unix)]
#[test]
fn a_restored_file_gets_back_its_recorded_mode() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    std::fs::set_permissions(fx.golden(), std::fs::Permissions::from_mode(0o600)).unwrap();
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, None).unwrap();
    std::fs::remove_file(fx.golden()).unwrap();

    restore(&files_only(), &remote, None, &files()).unwrap();
    let mode = std::fs::metadata(fx.golden()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn a_dry_run_writes_nothing_but_still_proves_every_blob_decrypts() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    std::fs::remove_file(fx.golden()).unwrap();

    let dry = RestoreOptions {
        dry_run: true,
        ..files()
    };
    let stats = restore(&files_only(), &remote, Some(&key), &dry).unwrap();
    assert_eq!(stats.files, 1);
    assert!(
        !fx.golden().exists(),
        "--dry-run must not touch the filesystem"
    );

    let golden = manifest_of(&remote, &generation)
        .files
        .into_iter()
        .find(|f| f.path.ends_with("golden.toml"))
        .unwrap();
    let object = blob_key(Some(key.id().as_str()), &golden.hash);
    let mut sealed = remote.get_bytes(&object).unwrap();
    let last = sealed.len() - 1;
    sealed[last] ^= 1;
    remote.put_bytes(&object, &sealed).unwrap();
    assert!(
        restore(&files_only(), &remote, Some(&key), &dry).is_err(),
        "a dry run must fail on a blob that would not decrypt"
    );
}

#[test]
fn the_wrong_key_is_refused_before_anything_is_written() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, Some(&Key::generate())).unwrap();
    std::fs::remove_file(fx.golden()).unwrap();

    let err = restore(&files_only(), &remote, Some(&Key::generate()), &files())
        .unwrap_err()
        .to_string();
    assert!(err.contains("different key"), "got: {err}");
    assert!(!fx.golden().exists());
}

#[test]
fn an_unencrypted_backup_is_refused_when_a_key_is_configured_unless_explicitly_allowed() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, None).unwrap();
    std::fs::remove_file(fx.golden()).unwrap();
    let key = Key::generate();

    let err = format!(
        "{:#}",
        restore(&files_only(), &remote, Some(&key), &files()).unwrap_err()
    );
    assert!(err.contains("NOT encrypted"), "got: {err}");
    assert!(err.contains("--allow-unencrypted-manifest"), "got: {err}");
    assert!(
        !err.contains("--allow-unauthenticated-manifest"),
        "got: {err}"
    );
    assert!(!fx.golden().exists());

    let allowed = RestoreOptions {
        allow_unencrypted_manifest: true,
        ..files()
    };
    restore(&files_only(), &remote, Some(&key), &allowed).unwrap();
    assert_eq!(std::fs::read_to_string(fx.golden()).unwrap(), GOLDEN);
}

#[test]
fn one_bad_blob_means_no_file_is_written_at_all() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    std::fs::remove_file(fx.config()).unwrap();
    std::fs::remove_file(fx.golden()).unwrap();

    let golden = manifest_of(&remote, &generation)
        .files
        .into_iter()
        .find(|f| f.path.ends_with("golden.toml"))
        .unwrap();
    remote
        .delete(&blob_key(Some(key.id().as_str()), &golden.hash))
        .unwrap();

    assert!(restore(&files_only(), &remote, Some(&key), &files()).is_err());
    assert!(
        !fx.config().exists(),
        "config.toml sorts before the missing golden blob and must still not be written"
    );
}

#[test]
fn a_plain_blob_that_does_not_match_its_hash_is_refused() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, None).unwrap().generation;
    std::fs::remove_file(fx.golden()).unwrap();

    let golden = manifest_of(&remote, &generation)
        .files
        .into_iter()
        .find(|f| f.path.ends_with("golden.toml"))
        .unwrap();
    remote
        .put_bytes(&blob_key(None, &golden.hash), b"not what was backed up")
        .unwrap();

    let err = restore(&files_only(), &remote, None, &files())
        .unwrap_err()
        .to_string();
    assert!(err.contains("do not match"), "got: {err}");
    assert!(!fx.golden().exists());
}

#[test]
fn a_manifest_path_outside_the_config_directory_is_refused() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, None).unwrap().generation;
    let mut m = manifest_of(&remote, &generation);

    for hostile in [
        "config/../escaped.toml",
        "config/nested/golden.toml",
        "transcripts/a.jsonl",
        "config/env",
    ] {
        m.files[0].path = hostile.into();
        remote
            .put_bytes(&generation, &serde_json::to_vec(&m).unwrap())
            .unwrap();
        let opts = RestoreOptions {
            generation: Some(generation.clone()),
            force: true,
            ..files()
        };
        let err = restore(&files_only(), &remote, None, &opts)
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing"), "{hostile}: {err}");
    }
    assert!(!fx.data.parent().unwrap().join("escaped.toml").exists());
}

#[test]
fn an_explicit_generation_is_honoured_over_the_latest() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    let old = back_up(&files_only(), &remote, None).unwrap().generation;
    std::fs::write(fx.golden(), "[[case]]\nquery = \"newer\"\n").unwrap();
    back_up(&files_only(), &remote, None).unwrap();
    std::fs::remove_file(fx.golden()).unwrap();

    let pinned = RestoreOptions {
        generation: Some(old),
        ..files()
    };
    restore(&files_only(), &remote, None, &pinned).unwrap();
    assert_eq!(std::fs::read_to_string(fx.golden()).unwrap(), GOLDEN);
}

#[test]
fn a_generation_that_does_not_exist_fails_by_name() {
    let _fx = Fixture::new();
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, None).unwrap();
    let missing = RestoreOptions {
        generation: Some("manifest/1999-01-01T00-00-00.000Z.json".into()),
        ..files()
    };
    let err = restore(&files_only(), &remote, None, &missing)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1999-01-01"), "got: {err}");
}

#[test]
fn without_a_latest_pointer_the_newest_generation_is_found_by_listing() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, None).unwrap();
    std::fs::write(fx.golden(), "[[case]]\nquery = \"newest\"\n").unwrap();
    let newest = back_up(&files_only(), &remote, None).unwrap().generation;
    remote.delete(Manifest::latest_key()).unwrap();

    assert_eq!(resolve_generation(&remote, None).unwrap(), newest);
    std::fs::remove_file(fx.golden()).unwrap();
    restore(&files_only(), &remote, None, &files()).unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.golden()).unwrap(),
        "[[case]]\nquery = \"newest\"\n"
    );
}

#[test]
fn an_empty_remote_says_there_is_nothing_to_restore() {
    let _fx = Fixture::new();
    let err = restore(&files_only(), &FakeRemote::new(), None, &files())
        .unwrap_err()
        .to_string();
    assert!(err.contains("no backups"), "got: {err}");
}

fn seed_index(db: &std::path::Path, contents: &[u8]) {
    std::fs::create_dir_all(db).unwrap();
    std::fs::write(db.join("graph.kz"), contents).unwrap();
    std::fs::write(db.join("graph.kz.wal"), b"wal").unwrap();
}

#[test]
fn the_index_round_trips_and_drops_the_stale_stamps() {
    for encrypted in [true, false] {
        let fx = Fixture::new();
        let key = Key::generate();
        let used = encrypted.then_some(&key);
        seed_index(&fx.db(), b"the index as it was backed up");
        let remote = FakeRemote::new();
        back_up(&with_index(), &remote, used).unwrap();

        std::fs::remove_dir_all(fx.db()).unwrap();
        seed_index(&fx.db(), b"an index built after the backup");
        std::fs::write(fx.db().join("only-in-the-newer-index"), b"x").unwrap();
        std::fs::write(fx.db().with_extension("stamps"), "{}").unwrap();

        let opts = RestoreOptions {
            index: true,
            ..files()
        };
        let stats = restore(&with_index(), &remote, used, &opts).unwrap();
        assert!(stats.index_restored, "encrypted: {encrypted}");
        assert_eq!(
            std::fs::read(fx.db().join("graph.kz")).unwrap(),
            b"the index as it was backed up".to_vec(),
            "encrypted: {encrypted}"
        );
        assert!(!fx.db().join("only-in-the-newer-index").exists());
        assert!(
            !fx.db().with_extension("stamps").exists(),
            "stamps describe the replaced index and would make the next run skip files"
        );
        assert!(!fx.db().with_extension("restore.staging").exists());
        assert!(!fx.db().with_extension("old").exists());
    }
}

#[test]
fn without_the_index_flag_the_live_index_is_untouched() {
    let fx = Fixture::new();
    seed_index(&fx.db(), b"backed up");
    let remote = FakeRemote::new();
    back_up(&with_index(), &remote, None).unwrap();
    seed_index(&fx.db(), b"live");

    let stats = restore(&with_index(), &remote, None, &files()).unwrap();
    assert!(!stats.index_restored);
    assert_eq!(
        std::fs::read(fx.db().join("graph.kz")).unwrap(),
        b"live".to_vec()
    );
}

#[test]
fn a_dry_run_leaves_the_index_alone() {
    let fx = Fixture::new();
    seed_index(&fx.db(), b"backed up");
    let remote = FakeRemote::new();
    back_up(&with_index(), &remote, None).unwrap();
    seed_index(&fx.db(), b"live");

    let opts = RestoreOptions {
        index: true,
        dry_run: true,
        ..files()
    };
    let stats = restore(&with_index(), &remote, None, &opts).unwrap();
    assert!(!stats.index_restored);
    assert_eq!(
        std::fs::read(fx.db().join("graph.kz")).unwrap(),
        b"live".to_vec()
    );
}

#[test]
fn restoring_the_index_while_indexing_runs_is_refused_and_changes_nothing() {
    let fx = Fixture::new();
    seed_index(&fx.db(), b"backed up");
    let remote = FakeRemote::new();
    back_up(&with_index(), &remote, None).unwrap();
    seed_index(&fx.db(), b"live");

    let _indexing = rhebok::index::IndexLock::acquire(&fx.db()).unwrap();
    let opts = RestoreOptions {
        index: true,
        ..files()
    };
    let err = restore(&with_index(), &remote, None, &opts)
        .unwrap_err()
        .to_string();
    assert!(err.contains("rhebok index"), "got: {err}");
    assert_eq!(
        std::fs::read(fx.db().join("graph.kz")).unwrap(),
        b"live".to_vec()
    );
    assert!(!fx.db().with_extension("restore.staging").exists());
}

#[test]
fn a_substituted_index_snapshot_is_refused_and_the_live_index_survives() {
    let fx = Fixture::new();
    seed_index(&fx.db(), b"backed up");
    let remote = FakeRemote::new();
    let generation = back_up(&with_index(), &remote, None).unwrap().generation;
    seed_index(&fx.db(), b"live");

    let ix = manifest_of(&remote, &generation).index.unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let other_db = elsewhere.path().join("db");
    seed_index(&other_db, b"a different, perfectly valid index");
    let substitute = rhebok::backup::snapshot::archive_index(&other_db, elsewhere.path())
        .unwrap()
        .unwrap();
    remote
        .put_file(&index_key(None, &ix.hash), &substitute.path)
        .unwrap();
    let opts = RestoreOptions {
        index: true,
        ..files()
    };
    assert!(restore(&with_index(), &remote, None, &opts).is_err());
    assert_eq!(
        std::fs::read(fx.db().join("graph.kz")).unwrap(),
        b"live".to_vec()
    );
}

#[test]
fn an_incompatible_index_is_refused_before_any_file_is_written() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, None).unwrap().generation;
    let mut m = manifest_of(&remote, &generation);
    m.index = Some(IndexEntry {
        hash: "abc".into(),
        size: 1,
        embed_model: "nomic-embed-text".into(),
        dimensions: 768,
        chunk_tokens: 512,
        documents: 1,
        chunks: 1,
    });
    remote
        .put_bytes(&generation, &serde_json::to_vec(&m).unwrap())
        .unwrap();
    std::fs::remove_file(fx.golden()).unwrap();

    let opts = RestoreOptions {
        index: true,
        ..files()
    };
    let err = restore(&Config::default(), &remote, None, &opts)
        .unwrap_err()
        .to_string();
    assert!(err.contains("nomic-embed-text"), "got: {err}");
    assert!(err.contains("--reindex"), "must say how to recover: {err}");
    assert!(!fx.golden().exists());
}

type ManifestEdit = fn(&mut serde_json::Value);

fn tampered(remote: &FakeRemote, generation: &str, edit: impl Fn(&mut serde_json::Value)) {
    let mut doc: serde_json::Value =
        serde_json::from_slice(&remote.get_bytes(generation).unwrap()).unwrap();
    edit(&mut doc);
    remote
        .put_bytes(generation, &serde_json::to_vec_pretty(&doc).unwrap())
        .unwrap();
}

#[test]
fn an_encrypted_backup_writes_an_authenticated_manifest() {
    let _fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    let auth = manifest_of(&remote, &generation).auth.unwrap();
    assert_eq!(auth.algo, "hmac-sha256");
    assert_eq!(auth.tag.len(), 64);

    let plain = FakeRemote::new();
    let generation = back_up(&files_only(), &plain, None).unwrap().generation;
    assert!(manifest_of(&plain, &generation).auth.is_none());
}

#[test]
fn a_manifest_altered_after_it_was_written_is_refused_and_nothing_is_restored() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    std::fs::remove_file(fx.golden()).unwrap();

    let edits: [(&str, ManifestEdit); 3] = [
        ("a file's mode", |doc| {
            doc["files"][0]["mode"] = 0o777.into()
        }),
        ("the host", |doc| doc["host"] = "elsewhere".into()),
        ("an added field", |doc| doc["extra"] = true.into()),
    ];
    let original = remote.get_bytes(&generation).unwrap();
    for (what, edit) in edits {
        remote.put_bytes(&generation, &original).unwrap();
        tampered(&remote, &generation, edit);
        let opts = RestoreOptions {
            allow_unauthenticated_manifest: true,
            allow_unencrypted_manifest: true,
            ..files()
        };
        let err = restore(&files_only(), &remote, Some(&key), &opts)
            .unwrap_err()
            .to_string();
        assert!(err.contains("fails authentication"), "{what}: {err}");
        assert!(!fx.golden().exists(), "{what}");
    }

    remote.put_bytes(&generation, &original).unwrap();
    restore(&files_only(), &remote, Some(&key), &files()).unwrap();
    assert_eq!(std::fs::read_to_string(fx.golden()).unwrap(), GOLDEN);
}

#[test]
fn stripping_the_encryption_claim_from_a_manifest_is_refused() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    std::fs::remove_file(fx.golden()).unwrap();
    tampered(&remote, &generation, |doc| {
        doc["encryption"] = serde_json::Value::Null;
        doc.as_object_mut().unwrap().remove("auth");
    });

    let err = restore(&files_only(), &remote, Some(&key), &files())
        .unwrap_err()
        .to_string();
    assert!(err.contains("NOT encrypted"), "got: {err}");
    assert!(!fx.golden().exists());
}

#[test]
fn a_manifest_from_before_authentication_is_refused_unless_explicitly_allowed() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    std::fs::remove_file(fx.golden()).unwrap();
    tampered(&remote, &generation, |doc| {
        let fields = doc.as_object_mut().unwrap();
        fields.remove("auth");
        fields.remove("memory");
        fields.insert("version".into(), 1.into());
    });

    let err = restore(&files_only(), &remote, Some(&key), &files())
        .unwrap_err()
        .to_string();
    assert!(err.contains("no authentication tag"), "got: {err}");
    assert!(
        err.contains("--allow-unauthenticated-manifest"),
        "got: {err}"
    );
    assert!(!fx.golden().exists());

    let allowed = RestoreOptions {
        allow_unauthenticated_manifest: true,
        ..files()
    };
    let stats = restore(&files_only(), &remote, Some(&key), &allowed).unwrap();
    assert_eq!(stats.memory, MemoryOutcome::NotInBackup);
    assert_eq!(std::fs::read_to_string(fx.golden()).unwrap(), GOLDEN);
}

#[test]
fn a_manifest_authenticated_under_another_key_with_a_forged_key_id_is_refused() {
    let fx = Fixture::new();
    let mine = Key::generate();
    let theirs = Key::generate();
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, Some(&theirs))
        .unwrap()
        .generation;
    std::fs::remove_file(fx.golden()).unwrap();
    tampered(&remote, &generation, |doc| {
        doc["encryption"]["key_id"] = mine.id().into();
    });

    let err = restore(&files_only(), &remote, Some(&mine), &files())
        .unwrap_err()
        .to_string();
    assert!(err.contains("fails authentication"), "got: {err}");
    assert!(!fx.golden().exists());
}

impl Fixture {
    fn memory(&self) -> PathBuf {
        self.data.join("memory")
    }

    fn memory_store(&self) -> PathBuf {
        rhebok::memory::store_dir(&self.memory())
    }

    fn memory_pack(&self) -> PathBuf {
        rhebok::memory::pack_dir(&self.memory())
    }
}

fn seed_memory(fx: &Fixture, store: &[u8], pack: Option<&[u8]>) {
    let _ = std::fs::remove_dir_all(fx.memory());
    std::fs::create_dir_all(fx.memory_store()).unwrap();
    std::fs::write(fx.memory_store().join("graph.kz"), store).unwrap();
    if let Some(pack) = pack {
        std::fs::create_dir_all(fx.memory_pack()).unwrap();
        std::fs::write(fx.memory_pack().join("pack.rec"), pack).unwrap();
    }
}

fn read_memory_store(fx: &Fixture) -> Vec<u8> {
    std::fs::read(fx.memory_store().join("graph.kz")).unwrap()
}

#[test]
fn memories_are_backed_up_and_restored_when_the_store_is_gone() {
    for encrypted in [true, false] {
        let fx = Fixture::new();
        let key = Key::generate();
        let used = encrypted.then_some(&key);
        seed_memory(&fx, b"memories as backed up", Some(b"pack as backed up"));
        let remote = FakeRemote::new();
        let generation = back_up(&files_only(), &remote, used).unwrap().generation;

        let entry = manifest_of(&remote, &generation)
            .memory
            .expect("the manifest records the memory snapshot");
        let key_id = used.map(|k| k.id());
        assert!(remote
            .get_bytes(&memory_key(key_id.as_deref(), &entry.hash))
            .is_ok());

        std::fs::remove_dir_all(fx.memory()).unwrap();
        let stats = restore(&files_only(), &remote, used, &files()).unwrap();
        assert_eq!(
            stats.memory,
            MemoryOutcome::Restored,
            "encrypted: {encrypted}"
        );
        assert_eq!(read_memory_store(&fx), b"memories as backed up".to_vec());
        assert_eq!(
            std::fs::read(fx.memory_pack().join("pack.rec")).unwrap(),
            b"pack as backed up".to_vec()
        );
        assert!(!fx.memory().with_extension("restore.staging").exists());
    }
}

#[test]
fn force_leaves_an_existing_memory_store_alone_and_memories_replaces_it_keeping_the_local_one() {
    let fx = Fixture::new();
    seed_memory(&fx, b"backed up", None);
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, None).unwrap();
    seed_memory(&fx, b"written since", Some(b"a pack built since"));

    for opts in [
        files(),
        RestoreOptions {
            force: true,
            ..files()
        },
    ] {
        let stats = restore(&files_only(), &remote, None, &opts).unwrap();
        assert_eq!(
            stats.memory,
            MemoryOutcome::KeptLocal,
            "force: {}",
            opts.force
        );
        assert_eq!(read_memory_store(&fx), b"written since".to_vec());
    }

    let replacing = RestoreOptions {
        memories: true,
        ..files()
    };
    let stats = restore(&files_only(), &remote, None, &replacing).unwrap();
    let MemoryOutcome::Replaced { previous } = stats.memory else {
        panic!(
            "expected the memory store to be replaced, got {:?}",
            stats.memory
        );
    };
    assert_eq!(read_memory_store(&fx), b"backed up".to_vec());
    assert_eq!(
        std::fs::read(previous.join("graph.kz")).unwrap(),
        b"written since".to_vec(),
        "the memory store a restore replaced must survive beside the restored one"
    );
    assert!(previous.starts_with(fx.memory()));
    assert!(
        !fx.memory_pack().exists(),
        "a pack the backup did not have would describe memories that are gone"
    );
}

#[test]
fn a_dry_run_proves_the_memory_snapshot_decrypts_and_writes_nothing() {
    let fx = Fixture::new();
    let key = Key::generate();
    seed_memory(&fx, b"backed up", None);
    let remote = FakeRemote::new();
    let generation = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    std::fs::remove_dir_all(fx.memory()).unwrap();

    let dry = RestoreOptions {
        dry_run: true,
        ..files()
    };
    assert_eq!(
        restore(&files_only(), &remote, Some(&key), &dry)
            .unwrap()
            .memory,
        MemoryOutcome::Restored
    );
    assert!(!fx.memory_store().exists());

    let entry = manifest_of(&remote, &generation).memory.unwrap();
    let object = memory_key(Some(key.id().as_str()), &entry.hash);
    let mut sealed = remote.get_bytes(&object).unwrap();
    let last = sealed.len() - 1;
    sealed[last] ^= 1;
    remote.put_bytes(&object, &sealed).unwrap();
    let err = format!(
        "{:#}",
        restore(&files_only(), &remote, Some(&key), &dry).unwrap_err()
    );
    assert!(err.contains("memory snapshot"), "got: {err}");
    assert!(!fx.memory().with_extension("restore.staging").exists());
}

#[test]
fn a_backup_waits_for_no_memory_write_and_fails_loudly_while_one_holds_the_lock() {
    let fx = Fixture::new();
    seed_memory(&fx, b"mid-write", None);
    let _writing = rhebok::index::IndexLock::acquire(&fx.memory_store()).unwrap();
    let remote = FakeRemote::new();

    let err = format!("{:#}", back_up(&files_only(), &remote, None).unwrap_err());
    assert!(err.contains("memory store"), "got: {err}");
    assert!(
        remote.list("manifest/").unwrap().is_empty(),
        "no generation may claim a backup that skipped the memories"
    );
}

#[test]
fn restoring_memories_while_a_memory_write_holds_the_lock_changes_nothing() {
    let fx = Fixture::new();
    seed_memory(&fx, b"backed up", None);
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, None).unwrap();
    seed_memory(&fx, b"live", None);
    std::fs::write(fx.config(), "edited since the backup\n").unwrap();

    let _writing = rhebok::index::IndexLock::acquire(&fx.memory_store()).unwrap();
    let forced = RestoreOptions {
        force: true,
        memories: true,
        ..files()
    };
    let err = format!(
        "{:#}",
        restore(&files_only(), &remote, None, &forced).unwrap_err()
    );
    assert!(err.contains("memory store"), "got: {err}");
    assert_eq!(read_memory_store(&fx), b"live".to_vec());
    assert_eq!(
        std::fs::read_to_string(fx.config()).unwrap(),
        "edited since the backup\n",
        "a restore that cannot take the memory lock must write no file at all"
    );
}

#[test]
fn pruning_keeps_the_memory_snapshot_a_surviving_generation_needs_and_drops_the_rest() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    seed_memory(&fx, b"first", None);
    let first = back_up(&files_only(), &remote, None).unwrap().generation;
    seed_memory(&fx, b"second", None);
    let second = back_up(&files_only(), &remote, None).unwrap().generation;
    let old = manifest_of(&remote, &first).memory.unwrap();
    let current = manifest_of(&remote, &second).memory.unwrap();
    assert_ne!(old.hash, current.hash);

    let stats = rhebok::backup::prune(&remote, 1, 1).unwrap();
    assert_eq!(stats.memory_deleted, 1);
    assert!(remote.get_bytes(&memory_key(None, &old.hash)).is_err());
    assert!(remote.get_bytes(&memory_key(None, &current.hash)).is_ok());
}

#[test]
fn an_unchanged_memory_store_is_not_uploaded_twice() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    seed_memory(&fx, b"unchanged", Some(b"pack"));
    let first = back_up(&files_only(), &remote, Some(&key)).unwrap();
    let second = back_up(&files_only(), &remote, Some(&key)).unwrap();
    assert_eq!(first.uploaded, 3);
    assert_eq!(second.uploaded, 0);
    assert_eq!(second.deduped, 3);
}

#[test]
fn a_real_memory_store_survives_the_round_trip_and_stays_writable() {
    use rhebok::memory::{forget_at, list_at, remember_at, Filter, MemoryKind, Origin, Remember};
    let fx = Fixture::new();
    let mut cfg = files_only();
    cfg.embed.dimensions = 4;
    cfg.memory.duplicate_similarity = 1.1;
    let lesson = |text: &str| Remember {
        kind: MemoryKind::Lesson,
        text: text.into(),
        title: None,
        project: None,
        confidence: 100,
        origin: Origin::User,
        session: None,
        source_hash: None,
        source_stamp: None,
        created: None,
    };
    for text in [
        "Squash every pull request to one commit.",
        "Never put a prefix on a commit subject.",
    ] {
        remember_at(
            &fx.memory(),
            &cfg,
            Box::new(common::FakeEmbedder::default()),
            lesson(text),
        )
        .unwrap();
    }
    let before: Vec<String> = list_at(&fx.memory(), &Filter::default())
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(before.len(), 2);

    let key = Key::generate();
    let remote = FakeRemote::new();
    back_up(&cfg, &remote, Some(&key)).unwrap();
    std::fs::remove_dir_all(fx.memory()).unwrap();

    assert_eq!(
        restore(&cfg, &remote, Some(&key), &files()).unwrap().memory,
        MemoryOutcome::Restored
    );
    let restored = list_at(&fx.memory(), &Filter::default()).unwrap();
    let mut after: Vec<String> = restored.iter().map(|m| m.text.clone()).collect();
    let mut expected = before.clone();
    after.sort();
    expected.sort();
    assert_eq!(after, expected);

    forget_at(&fx.memory(), &cfg, &restored[0].id).unwrap();
    assert_eq!(list_at(&fx.memory(), &Filter::default()).unwrap().len(), 1);
}

#[test]
fn the_override_for_an_untagged_manifest_does_not_admit_an_unencrypted_one() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    seed_memory(&fx, b"chosen by whoever controls the bucket", None);
    back_up(&files_only(), &remote, None).unwrap();
    std::fs::remove_dir_all(fx.memory()).unwrap();
    std::fs::remove_file(fx.golden()).unwrap();

    let untagged_only = RestoreOptions {
        allow_unauthenticated_manifest: true,
        ..files()
    };
    let err = restore(&files_only(), &remote, Some(&key), &untagged_only)
        .unwrap_err()
        .to_string();
    assert!(err.contains("NOT encrypted"), "got: {err}");
    assert!(!fx.golden().exists());
    assert!(!fx.memory_store().exists());
}

#[test]
fn a_manifest_copied_under_a_newer_generations_name_is_refused() {
    let fx = Fixture::new();
    let key = Key::generate();
    let remote = FakeRemote::new();
    let old = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    std::fs::write(fx.golden(), "[[case]]\nquery = \"newer\"\n").unwrap();
    let new = back_up(&files_only(), &remote, Some(&key))
        .unwrap()
        .generation;
    assert_ne!(old, new);
    std::fs::remove_file(fx.golden()).unwrap();
    remote
        .put_bytes(&new, &remote.get_bytes(&old).unwrap())
        .unwrap();

    let err = restore(&files_only(), &remote, Some(&key), &files())
        .unwrap_err()
        .to_string();
    assert!(err.contains("another generation's name"), "got: {err}");
    assert!(!fx.golden().exists());
}

#[test]
fn a_latest_pointer_at_an_older_generation_is_restored_but_reported_with_its_date() {
    let fx = Fixture::new();
    let remote = FakeRemote::new();
    let old = back_up(&files_only(), &remote, None).unwrap().generation;
    back_up(&files_only(), &remote, None).unwrap();
    remote
        .put_bytes(
            Manifest::latest_key(),
            format!("{{\"generation\": \"{old}\"}}").as_bytes(),
        )
        .unwrap();
    std::fs::remove_file(fx.golden()).unwrap();

    let stats = restore(&files_only(), &remote, None, &files()).unwrap();
    assert_eq!(stats.generation, old);
    assert_eq!(stats.created_at, manifest_of(&remote, &old).created_at);
    assert_eq!(stats.newer_generations, 1);

    let pinned = RestoreOptions {
        generation: Some(old),
        ..files()
    };
    assert_eq!(
        restore(&files_only(), &remote, None, &pinned)
            .unwrap()
            .newer_generations,
        0,
        "a generation the user named is what they asked for"
    );
}

#[test]
fn memories_embedded_with_another_model_or_width_are_not_restored_and_the_rest_is() {
    let mut narrower = files_only();
    narrower.embed.dimensions = 4;
    let mut other_model = files_only();
    other_model.embed.model = "another-embedder".into();
    for (what, current) in [("width", narrower), ("model", other_model)] {
        let fx = Fixture::new();
        seed_memory(&fx, b"embedded with the defaults", None);
        let remote = FakeRemote::new();
        back_up(&files_only(), &remote, None).unwrap();
        std::fs::remove_dir_all(fx.memory()).unwrap();
        std::fs::remove_file(fx.golden()).unwrap();

        let stats = restore(&current, &remote, None, &files()).unwrap();
        let MemoryOutcome::Incompatible(reason) = &stats.memory else {
            panic!(
                "{what}: expected an incompatible memory store, got {:?}",
                stats.memory
            );
        };
        assert!(reason.contains("rhebok memory rebuild"), "{what}: {reason}");
        assert!(!fx.memory_store().exists(), "{what}");
        assert_eq!(
            std::fs::read_to_string(fx.golden()).unwrap(),
            GOLDEN,
            "{what}"
        );
    }
}

#[test]
fn memories_are_checked_against_the_config_the_same_restore_brings_back() {
    let fx = Fixture::new();
    seed_memory(&fx, b"embedded at the default width", None);
    let remote = FakeRemote::new();
    back_up(&files_only(), &remote, None).unwrap();
    std::fs::remove_dir_all(fx.memory()).unwrap();
    std::fs::remove_file(fx.config()).unwrap();
    let mut before_the_config_returns = files_only();
    before_the_config_returns.embed.dimensions = 4;

    let stats = restore(&before_the_config_returns, &remote, None, &files()).unwrap();
    assert_eq!(stats.memory, MemoryOutcome::Restored);
    assert_eq!(
        read_memory_store(&fx),
        b"embedded at the default width".to_vec()
    );
}
