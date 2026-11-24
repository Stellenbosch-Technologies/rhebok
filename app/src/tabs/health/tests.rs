use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, size, AppContext, Bounds, Entity, Point, SharedString, TestAppContext, WindowBounds,
    WindowHandle, WindowOptions,
};
use rhebok::dashboard::core::embed::EmbedTest;
use rhebok::dashboard::core::index::{IndexRun, IndexRunView, IndexStarted};
use rhebok::dashboard::core::install::{EmbedView, InstallCheckView, InstallConfig, InstallView};
use rhebok::dashboard::core::stats::{Progress, RunningProgress, Stats, Tiers};
use rhebok::dashboard::core::version::Version;
use rhebok::dashboard::core::DashboardError;
use rhebok::setup::install::Fix;
use rhebok::update::Status;

use rhebok::bench::BenchTier;
use rhebok::dashboard::core::config::ConfigView;
use rhebok::dashboard::core::ignore::IgnorePreview;
use rhebok::skip::{Skip, SkipKind};

use super::ignore_editor::{short_path, suggestions_for};
use super::index_panel::{progress_line, running_line, IndexState, IndexStatus};
use super::install_card::embed_outcome;
use super::retrieval_panel::{threshold_label, to_fixed_2};
use crate::backend::{self, FixtureBackend};
use crate::shell::{Shell, TabId};
use crate::theme;
use crate::version_gate::Gate;
use crate::widgets::tail::{tail_lefts, tail_that_fits, ELLIPSIS};

fn stats() -> Stats {
    Stats {
        documents: 4,
        chunks: 5,
        by_source: HashMap::from([("markdown".to_string(), 4)]),
        sessions_by_agent: BTreeMap::new(),
        model: Some("qwen3-embedding:0.6b@abc".to_string()),
        skipped: Vec::new(),
        skipped_ignored: 0,
        bench: None,
        bench_provenance: None,
        bench_at: None,
        tiers: Tiers { hook: 1, mcp: 2 },
        hook_threshold: 0.7,
        memory: BTreeMap::new(),
    }
}

fn fixture() -> Arc<FixtureBackend> {
    let fixture = FixtureBackend::default();
    fixture.stats.set(Ok(stats()));
    Arc::new(fixture)
}

fn running(pct: f64) -> Progress {
    Progress::Running(RunningProgress {
        pct,
        docs_done: 1,
        docs_total: 4,
        chunks: 2,
        bytes_done: 10,
        bytes_total: 40,
        elapsed_s: 3,
        eta_s: 170,
        pid: 42,
    })
}

#[test]
fn a_running_index_rounds_its_percent_half_up_like_to_fixed() {
    let Progress::Running(p) = running(12.25) else {
        unreachable!()
    };
    assert_eq!(progress_line(&p), "12.3%, 1/4 documents, about 3 min left");
    assert_eq!(running_line(&p), "12.3% · 1/4 docs");
}

fn version() -> Version {
    Version {
        installed: "0.2.0".to_string(),
        latest: None,
        url: None,
        checked_at: None,
        error: None,
        no_release: false,
        update_available: None,
        update: None,
    }
}

fn status(done: bool, ok: Option<bool>, message: &str) -> Status {
    Status {
        pid: 7,
        started_at: 0,
        from: "0.2.0".to_string(),
        to: Some("0.3.0".to_string()),
        phase: "download".to_string(),
        message: message.to_string(),
        done,
        ok,
    }
}

fn check(name: &'static str, ok: bool, fix: Option<Fix>) -> InstallCheckView {
    InstallCheckView {
        name,
        ok,
        detail: format!("{name} detail"),
        fix,
    }
}

fn install(checks: Vec<InstallCheckView>) -> InstallView {
    InstallView {
        root: PathBuf::from("/data/rhebok"),
        binary: PathBuf::from("/data/rhebok/bin/rhebok"),
        exe: PathBuf::from("/data/rhebok/bin/rhebok"),
        checks,
        config: InstallConfig {
            path: PathBuf::from("/data/rhebok/config.toml"),
            errors: Vec::new(),
        },
        embed: EmbedView {
            backend: "ollama",
            url: Some("http://127.0.0.1:11434".to_string()),
            model: Some("qwen3-embedding:0.6b".to_string()),
        },
    }
}

fn open(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
    width: f32,
) -> (WindowHandle<Root>, Entity<Shell>) {
    open_sized(cx, fixture, width, 900.)
}

fn open_sized(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
    width: f32,
    height: f32,
) -> (WindowHandle<Root>, Entity<Shell>) {
    cx.update(|cx| {
        if !cx.has_global::<theme::Tokens>() {
            theme::install(cx);
        }
        backend::install(cx, fixture);
        let (window, shell) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(width), px(height)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| Shell::new(Gate::Ok, cx)),
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Base Root"), shell)
    })
}

fn click(cx: &mut TestAppContext, window: WindowHandle<Root>, id: impl Into<SharedString>) {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn click_now(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

fn label(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> Option<String> {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(id)
            .map(|e| e.label().unwrap_or_default().to_string())
    })
    .unwrap()
}

fn exists(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> bool {
    label(cx, window, id).is_some()
}

fn wait(cx: &mut TestAppContext, total: Duration) {
    let step = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    while waited < total {
        cx.executor().advance_clock(step);
        cx.run_until_parked();
        waited += step;
    }
}

fn run_background_only(cx: &mut TestAppContext) {
    let executor = cx.executor();
    let dispatcher = executor.dispatcher().as_test().expect("test dispatcher");
    while dispatcher.tick(true) {}
}

fn show_health(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
) -> (WindowHandle<Root>, Entity<Shell>) {
    let (window, shell) = open(cx, fixture, 1400.);
    cx.run_until_parked();
    click(cx, window, TabId::Health.button_id());
    (window, shell)
}

#[gpui_kit::test]
fn health_stops_polling_when_hidden(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.version.set(Ok(version()));
    let (window, _) = open(cx, fixture.clone(), 1400.);
    cx.run_until_parked();
    wait(cx, Duration::from_secs(3));
    assert_eq!(
        fixture.progress.calls(),
        0,
        "Health polled before it was shown"
    );

    click(cx, window, TabId::Health.button_id());
    wait(cx, Duration::from_secs(5));
    let shown = fixture.progress.calls();
    assert!(
        (2..=3).contains(&shown),
        "{shown} progress calls in 5s shown"
    );
    let versions = fixture.version.calls();
    assert!(
        (2..=3).contains(&versions),
        "{versions} version calls in 5s shown"
    );

    click(cx, window, TabId::Graph.button_id());
    wait(cx, Duration::from_secs(5));
    assert_eq!(
        fixture.progress.calls(),
        shown,
        "Health polled progress while hidden"
    );
    assert_eq!(
        fixture.version.calls(),
        versions,
        "Health polled version while hidden"
    );

    click(cx, window, TabId::Health.button_id());
    assert_eq!(
        fixture.progress.calls(),
        shown + 1,
        "showing Health again did not poll at once"
    );
}

#[gpui_kit::test]
fn the_install_card_lists_only_failed_checks_with_their_fix(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.install.set(install(vec![
        check("binary", true, None),
        check(
            "plugin",
            false,
            Some(Fix::Connect {
                agent: "claude-code",
            }),
        ),
        check(
            "marketplace",
            false,
            Some(Fix::Connect {
                agent: "claude-code",
            }),
        ),
        check(
            "path",
            false,
            Some(Fix::Command {
                command: "export PATH=/data/rhebok/bin:$PATH".to_string(),
            }),
        ),
        check(
            "claude",
            false,
            Some(Fix::Manual {
                text: "install Claude Code".to_string(),
            }),
        ),
    ]));
    let (window, _) = show_health(cx, fixture.clone());

    assert_eq!(
        label(cx, window, "install-summary").as_deref(),
        Some("4 problems")
    );
    assert!(
        !exists(cx, window, "check-binary"),
        "a passing check is listed"
    );
    assert_eq!(
        label(cx, window, "check-plugin").as_deref(),
        Some("Plugin files")
    );
    assert_eq!(
        label(cx, window, "connect-claude-code").as_deref(),
        Some("Connect Claude Code")
    );
    assert_eq!(
        label(cx, window, "check-marketplace").as_deref(),
        Some("Claude Code marketplace")
    );
    assert_eq!(
        label(cx, window, "check-marketplace-fix").as_deref(),
        Some("the same Connect fixes this")
    );
    assert_eq!(
        label(cx, window, "check-path-fix").as_deref(),
        Some("export PATH=/data/rhebok/bin:$PATH")
    );
    assert_eq!(
        label(cx, window, "check-path-fix-copy").as_deref(),
        Some("copy")
    );
    assert_eq!(
        label(cx, window, "check-claude-fix").as_deref(),
        Some("install Claude Code")
    );

    click(cx, window, "check-path-fix-copy");
    assert_eq!(
        label(cx, window, "check-path-fix-copy").as_deref(),
        Some("copied")
    );
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
        Some("export PATH=/data/rhebok/bin:$PATH")
    );

    fixture
        .agent_connect
        .set(Err(DashboardError::Conflict("busy elsewhere".to_string())));
    click_now(cx, window, "connect-claude-code");
    assert_eq!(
        label(cx, window, "connect-claude-code").as_deref(),
        Some("Connecting…")
    );
    click_now(cx, window, "connect-claude-code");
    cx.run_until_parked();
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|r| r.starts_with("agent_connect"))
            .count(),
        1
    );
    assert_eq!(
        label(cx, window, "install-change").as_deref(),
        Some("busy elsewhere")
    );
    assert_eq!(
        fixture.install.calls(),
        2,
        "connect did not reload the checks"
    );

    fixture.install.set(install(vec![
        check("binary", true, None),
        check("path", true, None),
    ]));
    click(cx, window, "install-recheck");
    assert_eq!(
        label(cx, window, "install-summary").as_deref(),
        Some("all 2 checks pass")
    );
    assert!(!exists(cx, window, "check-plugin"));
}

#[gpui_kit::test]
fn the_embed_line_tests_once_on_first_show_and_again_on_demand(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.install.set(install(Vec::new()));
    fixture.embed_test.set(EmbedTest {
        ok: true,
        latency_ms: 5,
        dimensions: Some(512),
        backend: "ollama",
        url: None,
        model: None,
        error: None,
    });
    let (window, _) = open(cx, fixture.clone(), 1400.);
    cx.run_until_parked();
    assert_eq!(fixture.embed_test.calls(), 0);

    click(cx, window, TabId::Health.button_id());
    assert_eq!(fixture.embed_test.calls(), 1);
    assert_eq!(
        label(cx, window, "embed-status").as_deref(),
        Some("Ollama http://127.0.0.1:11434 · qwen3-embedding:0.6b · reachable, 512 dimensions in 5 ms")
    );

    click(cx, window, TabId::Graph.button_id());
    click(cx, window, TabId::Health.button_id());
    assert_eq!(
        fixture.embed_test.calls(),
        1,
        "showing Health again re-tested"
    );

    fixture.embed_test.set(EmbedTest {
        ok: false,
        latency_ms: 0,
        dimensions: None,
        backend: "ollama",
        url: None,
        model: None,
        error: Some("connection refused".to_string()),
    });
    click_now(cx, window, "embed-test");
    assert_eq!(
        label(cx, window, "embed-status").as_deref(),
        Some("Ollama http://127.0.0.1:11434 · qwen3-embedding:0.6b · testing…")
    );
    cx.run_until_parked();
    assert_eq!(fixture.embed_test.calls(), 2);
    assert_eq!(
        label(cx, window, "embed-status").as_deref(),
        Some("Ollama http://127.0.0.1:11434 · qwen3-embedding:0.6b · connection refused")
    );
}

#[gpui_kit::test]
fn index_now_disables_while_a_run_is_active(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.progress.set(Ok(running(37.5)));
    let (window, shell) = show_health(cx, fixture.clone());

    assert_eq!(label(cx, window, "index-now").as_deref(), Some("Indexing…"));
    assert_eq!(
        label(cx, window, "index-progress").as_deref(),
        Some("37.5%, 1/4 documents, about 3 min left")
    );
    click(cx, window, "index-now");
    assert_eq!(fixture.index_start.calls(), 0, "Index now ran during a run");

    let stats_before = fixture.stats.calls();
    fixture.progress.set(Ok(Progress::Idle { idle: true }));
    wait(cx, Duration::from_secs(2));
    assert_eq!(label(cx, window, "index-now").as_deref(), Some("Index now"));
    assert!(!exists(cx, window, "index-progress"));
    assert_eq!(
        fixture.stats.calls(),
        stats_before + 1,
        "a finished run did not refresh stats"
    );
    assert_eq!(shell.read_with(cx, |s, _| s.index_runs()), 1);

    fixture.index_start.set(Ok(IndexStarted {
        started: true,
        pid: 43,
        reindex: false,
    }));
    click_now(cx, window, "index-now");
    assert_eq!(label(cx, window, "index-now").as_deref(), Some("Starting…"));
    click_now(cx, window, "index-now");
    cx.run_until_parked();
    assert_eq!(
        fixture.index_start.calls(),
        1,
        "a second click started a second run"
    );
    assert_eq!(label(cx, window, "index-now").as_deref(), Some("Index now"));

    fixture.index_start.set(Err(DashboardError::Conflict(
        "an index is already running".to_string(),
    )));
    click(cx, window, "index-now");
    assert_eq!(
        label(cx, window, "index-note").as_deref(),
        Some("an index is already running")
    );
}

#[gpui_kit::test]
fn the_version_card_shows_each_of_its_states(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.version.set(Ok(version()));
    let (window, _) = show_health(cx, fixture.clone());

    let available = Version {
        update_available: Some("0.3.0".to_string()),
        latest: Some("0.3.0".to_string()),
        url: Some("https://example.invalid/notes".to_string()),
        ..version()
    };
    let cases: Vec<(&str, Version, &str, bool, bool)> = vec![
        (
            "updating",
            Version {
                update: Some(status(false, None, "fetching")),
                ..version()
            },
            "Updating to 0.3.0: download — fetching",
            false,
            false,
        ),
        (
            "updated",
            Version {
                update: Some(status(true, Some(true), "done")),
                ..version()
            },
            "Updated to 0.3.0. Restart Claude Code, and this dashboard: this page is still served by the old binary.",
            false,
            false,
        ),
        (
            "failed",
            Version {
                update: Some(status(true, Some(false), "checksum mismatch")),
                ..version()
            },
            "Update failed: checksum mismatch. See db.log beside the index.",
            true,
            true,
        ),
        (
            "available",
            available.clone(),
            "0.3.0 available, you have 0.2.0 · release notes",
            true,
            true,
        ),
        (
            "available, last check failed",
            Version {
                error: Some("rate limited".to_string()),
                ..available
            },
            "0.3.0 available, you have 0.2.0 · release notes · last check failed never: rate limited",
            true,
            true,
        ),
        (
            "check failed",
            Version {
                error: Some("offline".to_string()),
                ..version()
            },
            "update check failed never: offline",
            false,
            true,
        ),
        (
            "no release",
            Version {
                no_release: true,
                ..version()
            },
            "no release published yet · checked never",
            false,
            true,
        ),
        (
            "up to date",
            version(),
            "up to date · checked never",
            false,
            true,
        ),
    ];
    for (name, v, text, update, check) in cases {
        fixture.version.set(Ok(v));
        wait(cx, Duration::from_secs(2));
        assert_eq!(
            label(cx, window, "version-status").as_deref(),
            Some(text),
            "{name}"
        );
        assert_eq!(
            exists(cx, window, "version-update"),
            update,
            "{name}: Update button"
        );
        assert_eq!(
            exists(cx, window, "version-check"),
            check,
            "{name}: Check now button"
        );
    }

    fixture
        .version
        .set(Err(DashboardError::Unavailable("busy".to_string())));
    click(cx, window, "version-check");
    assert_eq!(
        label(cx, window, "version-error").as_deref(),
        Some("The index is busy — try again in a moment.")
    );
    assert!(fixture
        .requests()
        .contains(&"version refresh=true".to_string()));
}

#[gpui_kit::test]
fn the_update_button_waits_for_a_running_index(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.progress.set(Ok(running(10.4)));
    fixture.version.set(Ok(Version {
        update_available: Some("0.3.0".to_string()),
        ..version()
    }));
    let (window, _) = show_health(cx, fixture.clone());
    assert!(exists(cx, window, "version-update"));
    click(cx, window, "version-update");
    assert_eq!(
        fixture.update_start.calls(),
        0,
        "Update ran during an index"
    );
}

#[gpui_kit::test]
fn open_memories_switches_tab(cx: &mut TestAppContext) {
    let (window, shell) = show_health(cx, fixture());
    assert_eq!(label(cx, window, "memory-total").as_deref(), Some("0"));
    click(cx, window, "memory-open");
    assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Memories);
}

#[gpui_kit::test]
fn the_grid_stacks_below_1024_px(cx: &mut TestAppContext) {
    for (width, stacked) in [(1400., false), (1000., true)] {
        let (window, _) = open(cx, fixture(), width);
        cx.run_until_parked();
        click(cx, window, TabId::Health.button_id());
        let (index, memory) = cx
            .update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                (
                    window.find("index-panel").bounds(),
                    window.find("memory-panel").bounds(),
                )
            })
            .unwrap();
        if stacked {
            assert!(memory.origin.y > index.origin.y, "{width}: not stacked");
            assert_eq!(memory.size.width, index.size.width, "{width}");
        } else {
            assert_eq!(memory.origin.y, index.origin.y, "{width}: stacked");
            let ratio = index.size.width / memory.size.width;
            assert!((ratio - 2.).abs() < 0.01, "{width}: ratio {ratio}");
        }
    }
}

#[gpui_kit::test]
fn index_status_polls_only_while_a_run_is_active(cx: &mut TestAppContext) {
    let fixture = fixture();
    cx.update(|cx| {
        theme::install(cx);
        backend::install(cx, fixture.clone());
    });
    let status = cx.new(|_| IndexStatus::new("settings-index-status"));
    wait(cx, Duration::from_secs(2));
    assert_eq!(
        fixture.index_status.calls(),
        0,
        "an idle IndexStatus polled"
    );

    fixture.index_start.set(Ok(IndexStarted {
        started: true,
        pid: 51,
        reindex: true,
    }));
    fixture.progress.set(Ok(running(50.6)));
    status.update(cx, |s, cx| s.start(true, cx));
    cx.run_until_parked();
    wait(cx, Duration::from_millis(2100));
    let polled = fixture.index_status.calls();
    assert!(
        (3..=4).contains(&polled),
        "{polled} index_status calls in 2.1s"
    );
    assert!(status.read_with(cx, |s, _| matches!(
        s.state(),
        IndexState::Running {
            progress: Progress::Running(_),
            ..
        }
    )));
    status.update(cx, |s, cx| s.start(false, cx));
    assert_eq!(
        fixture.index_start.calls(),
        1,
        "a second start during a run"
    );

    fixture.index_status.set(IndexRunView {
        run: Some(IndexRun {
            pid: 51,
            reindex: true,
            finished: true,
            ok: Some(false),
            code: Some(2),
            log: "/data/db.log".to_string(),
        }),
    });
    wait(cx, Duration::from_millis(800));
    let finished = fixture.index_status.calls();
    status.read_with(cx, |s, _| match s.state() {
        IndexState::Failed { reason } => assert_eq!(
            reason,
            "The index failed: rhebok index exited with code 2; its output is in /data/db.log"
        ),
        other => panic!("expected a failure, got {other:?}"),
    });
    wait(cx, Duration::from_secs(3));
    assert_eq!(
        fixture.index_status.calls(),
        finished,
        "IndexStatus polled after the run ended"
    );
}

#[gpui_kit::test]
fn a_slow_progress_call_is_not_replaced_by_the_next_tick(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (_, shell) = show_health(cx, fixture.clone());
    let health = shell
        .read_with(cx, |s, _| s.health.clone())
        .expect("Health opened");
    let before = fixture.progress.calls();
    assert!(!health.read_with(cx, |h, _| h.progress().is_running()));

    fixture.progress.set(Ok(running(12.3)));
    let release = fixture.progress.hold();
    release.send(()).unwrap();
    health.update(cx, |h, cx| h.poll_progress(cx));
    run_background_only(cx);
    assert_eq!(fixture.progress.calls(), before + 1);
    health.update(cx, |h, cx| h.poll_progress(cx));
    run_background_only(cx);
    cx.run_until_parked();
    assert_eq!(
        fixture.progress.calls(),
        before + 1,
        "the second tick started a second call"
    );
    assert!(
        health.read_with(cx, |h, _| h.progress().is_running()),
        "the held call's result did not land"
    );
    health.update(cx, |h, cx| h.poll_progress(cx));
    cx.run_until_parked();
    assert_eq!(
        fixture.progress.calls(),
        before + 2,
        "polling stayed blocked after the call landed"
    );
}

#[test]
fn a_missing_dimension_prints_nothing_like_the_web() {
    let test = EmbedTest {
        ok: true,
        latency_ms: 5,
        dimensions: None,
        backend: "ollama",
        url: None,
        model: None,
        error: None,
    };
    assert_eq!(
        embed_outcome(Some(&test), false).as_deref(),
        Some(" · reachable,  dimensions in 5 ms")
    );
}

#[gpui_kit::test]
fn a_slow_version_call_is_not_replaced_by_the_next_tick(cx: &mut TestAppContext) {
    let fixture = fixture();
    cx.update(|cx| {
        theme::install(cx);
        backend::install(cx, fixture.clone());
    });
    let card = cx.new(|_| super::version_card::VersionCard::new());
    fixture.version.set(Ok(version()));
    let release = fixture.version.hold();
    release.send(()).unwrap();
    card.update(cx, |c, cx| c.tick(cx));
    run_background_only(cx);
    assert_eq!(fixture.version.calls(), 1);
    card.update(cx, |c, cx| c.tick(cx));
    run_background_only(cx);
    cx.run_until_parked();
    assert_eq!(
        fixture.version.calls(),
        1,
        "the second tick started a second call"
    );
    assert!(
        card.read_with(cx, |c, _| c.has_version()),
        "the held call's result did not land"
    );
    card.update(cx, |c, cx| c.tick(cx));
    cx.run_until_parked();
    assert_eq!(
        fixture.version.calls(),
        2,
        "polling stayed blocked after the call landed"
    );
}

#[gpui_kit::test]
fn a_slow_index_status_call_is_not_replaced_by_the_next_tick(cx: &mut TestAppContext) {
    let fixture = fixture();
    cx.update(|cx| {
        theme::install(cx);
        backend::install(cx, fixture.clone());
    });
    let status = cx.new(|_| IndexStatus::new("index-status"));
    fixture.index_start.set(Ok(IndexStarted {
        started: true,
        pid: 61,
        reindex: false,
    }));
    status.update(cx, |s, cx| s.start(false, cx));
    cx.run_until_parked();
    let before = fixture.index_status.calls();
    fixture.progress.set(Ok(running(42.7)));
    let release = fixture.index_status.hold();
    release.send(()).unwrap();
    status.update(cx, |s, cx| s.tick(61, cx));
    run_background_only(cx);
    assert_eq!(fixture.index_status.calls(), before + 1);
    status.update(cx, |s, cx| s.tick(61, cx));
    run_background_only(cx);
    cx.run_until_parked();
    assert_eq!(
        fixture.index_status.calls(),
        before + 1,
        "the second tick started a second call"
    );
    assert!(
        status.read_with(cx, |s, _| matches!(
            s.state(),
            IndexState::Running {
                progress: Progress::Running(_),
                ..
            }
        )),
        "the held call's result did not land"
    );
    status.update(cx, |s, cx| s.tick(61, cx));
    cx.run_until_parked();
    assert_eq!(
        fixture.index_status.calls(),
        before + 2,
        "polling stayed blocked after the call landed"
    );
}

fn bench_tier(tier: &str, recall: f32, p50: u64, p95: u64) -> BenchTier {
    BenchTier {
        tier: tier.to_string(),
        recall_at_5: recall,
        p50_ms: p50,
        p95_ms: p95,
        absent: Vec::new(),
    }
}

fn benched() -> Stats {
    Stats {
        bench: Some(vec![
            bench_tier("instant", 0.7, 54, 80),
            bench_tier("fast", 0.9, 101, 180),
            bench_tier("balanced", 0.92, 204, 300),
            bench_tier("thorough", 0.92, 231, 340),
            bench_tier("exhaustive", 0.93, 338, 512),
        ]),
        bench_at: Some(1),
        ..stats()
    }
}

fn skip(path: &str, kind: SkipKind) -> Skip {
    Skip {
        path: path.to_string(),
        kind,
        detail: "empty transcript".to_string(),
        io: None,
    }
}

fn skipped_fixture() -> Arc<FixtureBackend> {
    let fixture = FixtureBackend::default();
    fixture.stats.set(Ok(Stats {
        skipped: vec![
            skip(
                "/Users/me/.claude/projects/-Users-me-dev-app/a.jsonl",
                SkipKind::Empty,
            ),
            skip(
                "/Users/me/.claude/projects/-Users-me-dev-app/b.jsonl",
                SkipKind::Empty,
            ),
        ],
        ..stats()
    }));
    fixture.ignore_preview.set(Ok(IgnorePreview {
        error: None,
        matches: 2,
        of: 2,
        sample: Vec::new(),
    }));
    fixture.config_view.set(Ok(ConfigView(serde_json::json!({
        "etag": "e1",
        "effective": { "ignore": ["drafts"] }
    }))));
    Arc::new(fixture)
}

fn show_tall(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
) -> (WindowHandle<Root>, Entity<Shell>) {
    let (window, shell) = open_sized(cx, fixture, 1400., 2400.);
    cx.run_until_parked();
    click(cx, window, TabId::Health.button_id());
    (window, shell)
}

fn setting(cx: &mut TestAppContext, shell: &Entity<Shell>) -> Option<String> {
    shell.read_with(cx, |s, _| s.setting_request().map(|r| r.key.clone()))
}

#[gpui_kit::test]
fn clicking_a_tier_row_shows_its_detail(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.stats.set(Ok(benched()));
    let (window, shell) = show_tall(cx, fixture);

    let detail = label(cx, window, "tier-detail").expect("tier detail");
    assert_eq!(
        detail,
        "At fast, 90% of the test questions that have an answer found it in the top five results. Half the searches finished within 101 ms and 95% within 180 ms."
    );
    assert!(!exists(cx, window, "tier-change-hook"));
    assert!(exists(cx, window, "tier-change-mcp"));

    click(cx, window, "tier-thorough");
    assert_eq!(
        label(cx, window, "tier-detail").as_deref(),
        Some("At thorough, 92% of the test questions that have an answer found it in the top five results. Half the searches finished within 231 ms and 95% within 340 ms. Against fast, the hook's tier, that is +2 points of recall for +130 ms.")
    );
    click(cx, window, "tier-instant");
    assert!(label(cx, window, "tier-detail")
        .unwrap()
        .ends_with("that is -20 points of recall for -47 ms."));

    click(cx, window, "tier-change-hook");
    assert_eq!(setting(cx, &shell).as_deref(), Some("hook.quality"));
    assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Settings);
    click(cx, window, TabId::Health.button_id());
    click(cx, window, "tier-change-mcp");
    assert_eq!(setting(cx, &shell).as_deref(), Some("mcp.quality"));
}

#[gpui_kit::test]
fn the_threshold_link_opens_the_hook_threshold_setting(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.stats.set(Ok(benched()));
    let (window, shell) = show_tall(cx, fixture);
    assert_eq!(label(cx, window, "hook-threshold").as_deref(), Some("0.70"));
    click(cx, window, "hook-threshold");
    assert_eq!(setting(cx, &shell).as_deref(), Some("hook.threshold"));
    assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Settings);
}

#[gpui_kit::test]
fn the_retrieval_panel_says_how_to_populate_it(cx: &mut TestAppContext) {
    let (window, _) = show_tall(cx, fixture());
    assert!(exists(cx, window, "retrieval-panel"));
    assert!(!exists(cx, window, "tier-detail"));
    assert_eq!(
        label(cx, window, "retrieval-empty-copy").as_deref(),
        Some("copy")
    );
    assert_eq!(label(cx, window, "skipped-count").as_deref(), Some("none"));
    assert_eq!(
        label(cx, window, "skipped-empty").as_deref(),
        Some("The last index run read every file it found.")
    );
}

#[test]
fn a_long_path_keeps_its_tail_visible() {
    let width = |s: &str| px(7. * s.chars().count() as f32);
    let path = "sessions/2026/11/22/very/deep/folder/rollout-final.jsonl";
    assert_eq!(tail_that_fits(path, px(1000.), width), None);
    let room = px(140.);
    let tail = tail_that_fits(path, room, width).expect("the path is too long to fit");
    assert!(path.ends_with(&tail), "{tail} is not the end of the path");
    assert!(tail.ends_with("rollout-final.jsonl"), "{tail}");
    let shown = format!("{ELLIPSIS}{tail}");
    assert!(width(&shown) <= room, "{shown} overflows");
    let one_more = &path[path.len() - tail.len() - 1..];
    assert!(
        width(&format!("{ELLIPSIS}{one_more}")) > room,
        "{tail} dropped a character that fits"
    );
}

#[test]
fn the_ellipsis_touches_the_first_visible_glyph() {
    let (ellipsis, tail) = tail_lefts(px(300.), px(120.), px(7.));
    assert_eq!(tail, px(180.));
    assert_eq!(ellipsis, px(173.));
}

#[test]
fn the_threshold_rounds_half_up_like_to_fixed() {
    assert_eq!(to_fixed_2(0.625), "0.63");
    assert_eq!(to_fixed_2(0.125), "0.13");
    assert_eq!(to_fixed_2(0.0), "0.00");
    assert_eq!(threshold_label(0.66), "0.66");
    assert_eq!(threshold_label(0.7), "0.70");
}

#[gpui_kit::test]
fn the_ignore_preview_debounces(cx: &mut TestAppContext) {
    let fixture = skipped_fixture();
    let (window, _) = show_tall(cx, fixture.clone());
    click(cx, window, "skip-group-empty-ignore");
    assert!(exists(cx, window, "ignore-editor"));
    assert_eq!(
        label(cx, window, "ignore-entry").as_deref(),
        Some("Ignore entry")
    );
    wait(cx, Duration::from_millis(300));
    assert_eq!(fixture.ignore_preview.calls(), 1);
    assert!(fixture
        .requests()
        .contains(&"ignore_preview ~/.claude/projects/-Users-me-dev-app".to_string()));
    assert_eq!(
        label(cx, window, "ignore-preview").as_deref(),
        Some("Hides 2 of the 2 skipped files listed here. Notes and sessions it matches are no longer indexed either.")
    );

    click(cx, window, "ignore-entry");
    for key in ["a", "b", "c", "d", "e"] {
        cx.update_window(window.into(), |_, window, cx| window.input(key, cx))
            .unwrap();
        cx.executor().advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
    }
    assert_eq!(fixture.ignore_preview.calls(), 1, "previewed mid-typing");
    cx.executor().advance_clock(Duration::from_millis(220));
    cx.run_until_parked();
    assert_eq!(
        fixture.ignore_preview.calls(),
        1,
        "previewed 240 ms after the last keystroke"
    );
    wait(cx, Duration::from_millis(300));
    assert_eq!(fixture.ignore_preview.calls(), 2);
    assert!(fixture
        .requests()
        .iter()
        .any(|r| r.ends_with("dev-appabcde")));
}

#[gpui_kit::test]
fn adding_a_pattern_retries_once_on_a_config_conflict(cx: &mut TestAppContext) {
    let fixture = skipped_fixture();
    fixture.config_save.set(Err(DashboardError::ConfigChanged {
        message: "changed".to_string(),
        etag: "e2".to_string(),
    }));
    let (window, _) = show_tall(cx, fixture.clone());
    click(cx, window, "skip-group-empty-ignore");
    wait(cx, Duration::from_millis(300));
    click(cx, window, "ignore-add");
    assert_eq!(fixture.config_view.calls(), 2);
    assert_eq!(fixture.config_save.calls(), 2);
    assert_eq!(
        label(cx, window, "ignore-error").as_deref(),
        Some("config.toml changed while saving. Try again.")
    );
    assert!(fixture
        .requests()
        .iter()
        .any(|r| r.contains("\"drafts\"") && r.contains("~/.claude/projects/-Users-me-dev-app")));
}

#[gpui_kit::test]
fn adding_a_pattern_shows_the_added_banner_and_refreshes(cx: &mut TestAppContext) {
    let fixture = skipped_fixture();
    fixture
        .config_save
        .set(Ok(ConfigView(serde_json::json!({ "etag": "e2" }))));
    let (window, shell) = show_tall(cx, fixture.clone());
    let before = fixture.stats.calls();
    click(cx, window, "skip-group-empty-ignore");
    wait(cx, Duration::from_millis(300));
    click(cx, window, "ignore-add");
    assert_eq!(fixture.config_save.calls(), 1);
    assert!(!exists(cx, window, "ignore-editor"));
    assert_eq!(
        label(cx, window, "ignore-added").as_deref(),
        Some("Added ~/.claude/projects/-Users-me-dev-app to the ignore list. Matching files are hidden here now, and the next index does not read them.")
    );
    assert!(fixture.stats.calls() > before, "stats were not refreshed");
    click(cx, window, "ignore-added-edit");
    assert_eq!(setting(cx, &shell).as_deref(), Some("ignore"));
}

#[test]
fn suggestions_match_the_web() {
    let one = [skip(
        "/Users/me/.claude/projects/-Users-me/subagents/workflows/w1/agent.jsonl",
        SkipKind::Empty,
    )];
    let labels: Vec<(String, String)> = suggestions_for(&one)
        .into_iter()
        .map(|s| (s.label, s.pattern))
        .collect();
    assert_eq!(
        labels,
        vec![
            (
                "this file".to_string(),
                "~/.claude/projects/-Users-me/subagents/workflows/w1/agent.jsonl".to_string()
            ),
            (
                "its folder".to_string(),
                "~/.claude/projects/-Users-me/subagents/workflows/w1".to_string()
            ),
            (
                "every workflow agent.jsonl".to_string(),
                "subagents/workflows/*/agent.jsonl".to_string()
            ),
            (
                "every file named agent.jsonl".to_string(),
                "agent.jsonl".to_string()
            ),
        ]
    );
    let shown = short_path("/Users/me/.claude/projects/-Users-me-dev-app/x/a.jsonl");
    assert_eq!(shown.0.as_deref(), Some("dev-app"));
    assert_eq!(shown.1, "x/a.jsonl");
}

#[gpui_kit::test]
fn a_copied_path_is_the_original_text(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.install.set(install(vec![check(
        "path",
        false,
        Some(Fix::Command {
            command: "export PATH=/data/rhebok/bin:$PATH".to_string(),
        }),
    )]));
    let (window, _) = show_health(cx, fixture);
    click(cx, window, "check-path-fix-copy");
    let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(
        copied.as_deref(),
        Some("export PATH=/data/rhebok/bin:$PATH")
    );
}

#[test]
fn to_fixed_prints_what_js_prints_for_non_numbers() {
    assert_eq!(to_fixed_2(f64::NAN), "NaN");
    assert_eq!(to_fixed_2(f64::INFINITY), "Infinity");
    assert_eq!(to_fixed_2(f64::NEG_INFINITY), "-Infinity");
}
