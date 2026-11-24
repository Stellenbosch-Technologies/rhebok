use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, size, AppContext, Bounds, Entity, Point, SharedString, TestAppContext, WindowBounds,
    WindowHandle, WindowOptions,
};
use rhebok::dashboard::core::agents::{AgentChanged, AgentList};
use rhebok::dashboard::core::config::ConfigView;
use rhebok::dashboard::core::graph::{Graph, Node};
use rhebok::dashboard::core::index::{IndexRun, IndexRunView, IndexStarted};
use rhebok::dashboard::core::stats::{Stats, Tiers};
use rhebok::dashboard::core::DashboardError;
use rhebok::setup::agents::Change;
use serde_json::{json, Value};

use super::{
    folder_id, locale_compare, suggest_query, Onboarding, ADD_ID, CONFLICT, CONTINUE_ID,
    GENERIC_QUERY, INPUT_ID, PROBE_DELAY, RECOMMENDED, ROOT_ID, SAVE_ID, SKIP_ID, STEPS_ID,
    WITHOUT_SAVING_ID,
};
use crate::app_state::{self, FILE_NAME};
use crate::backend::{self, FixtureBackend};
use crate::shell::{SetupState, Shell, TabId, LOADING};
use crate::tabs::settings::sources::RELATIVE;
use crate::theme;
use crate::version_gate::Gate;
use crate::widgets::agent_card::{card_id, part_id};

fn stats(documents: i64) -> Stats {
    Stats {
        documents,
        chunks: documents * 3,
        by_source: HashMap::new(),
        sessions_by_agent: BTreeMap::new(),
        model: None,
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

fn config(sources: &[&str], etag: &str) -> ConfigView {
    ConfigView(json!({
        "path": "/home/u/.config/rhebok/config.toml",
        "exists": true,
        "etag": etag,
        "file": { "sources": sources },
        "effective": { "sources": sources, "index_transcripts": false },
        "defaults": { "sources": [] },
        "errors": [],
        "overrides": [],
    }))
}

fn agent(id: &str, name: &str, installed: bool) -> Value {
    json!({
        "id": id,
        "name": name,
        "detected": {
            "installed": installed,
            "version": if installed { json!("1.0.0") } else { Value::Null },
            "config_path": format!("/home/u/.{id}/config.json"),
        },
        "status": { "state": "not_connected" },
        "capabilities": {
            "mcp": true,
            "prompt_hook": false,
            "session_hook": false,
            "transcripts": false,
            "instructions": false,
        },
    })
}

fn agents() -> AgentList {
    AgentList(json!({
        "agents": [
            agent("claude-code", "Claude Code", true),
            agent("codex", "Codex", false),
            agent("cursor", "Cursor", true),
        ],
        "snippets": { "mcp_json": "{}", "codex_toml": "" },
    }))
}

fn fixture(documents: i64, sources: &[&str]) -> Arc<FixtureBackend> {
    let fixture = FixtureBackend::default();
    fixture.stats.set(Ok(stats(documents)));
    fixture.config_view.set(Ok(config(sources, "etag-1")));
    fixture.agents.set(agents());
    Arc::new(fixture)
}

fn open(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
    state: Option<&Path>,
) -> (WindowHandle<Root>, Entity<Shell>) {
    let state = state.map(Path::to_path_buf);
    let opened = cx.update(|cx| {
        if !cx.has_global::<theme::Tokens>() {
            theme::install(cx);
        }
        backend::install(cx, fixture);
        if let Some(path) = state {
            app_state::install(cx, path);
        }
        let (window, shell) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1400.), px(900.)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| Shell::new(Gate::Ok, cx)),
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Base Root"), shell)
    });
    cx.run_until_parked();
    render(cx, opened.0);
    opened
}

fn render(cx: &mut TestAppContext, window: WindowHandle<Root>) {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

fn click(cx: &mut TestAppContext, window: WindowHandle<Root>, id: impl Into<SharedString>) {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
    render(cx, window);
}

fn click_now(cx: &mut TestAppContext, window: WindowHandle<Root>, id: impl Into<SharedString>) {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn type_text(cx: &mut TestAppContext, window: WindowHandle<Root>, text: &str) {
    click(cx, window, INPUT_ID);
    cx.update_window(window.into(), |_, window, cx| {
        window.press("cmd-a", cx);
        window.input(text, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.executor().advance_clock(PROBE_DELAY);
    cx.run_until_parked();
    render(cx, window);
}

fn shows(cx: &mut TestAppContext, window: WindowHandle<Root>, id: impl Into<SharedString>) -> bool {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some_and(|e| e.visible())
    })
    .unwrap()
}

fn label(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    id: impl Into<SharedString>,
) -> Option<String> {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(id)
            .map(|e| e.label().unwrap_or_default().to_string())
    })
    .unwrap()
}

fn setup(cx: &mut TestAppContext, shell: &Entity<Shell>) -> SetupState {
    shell.read_with(cx, |s, _| s.setup())
}

fn onboarding(cx: &mut TestAppContext, shell: &Entity<Shell>) -> Entity<Onboarding> {
    shell
        .read_with(cx, |s, _| s.onboarding())
        .expect("onboarding is open")
}

fn no_tab_selected(cx: &mut TestAppContext, window: WindowHandle<Root>) -> bool {
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        TabId::ALL
            .iter()
            .all(|tab| window.find(tab.button_id()).selected() == Some(false))
    })
    .unwrap()
}

fn state_file(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("rhebok").join(FILE_NAME)
}

fn dismissed_on_disk(path: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text).ok()?["setup_dismissed"].as_bool()
}

#[gpui_kit::test]
fn onboarding_opens_only_on_an_empty_unindexed_unskipped_machine(cx: &mut TestAppContext) {
    let empty = fixture(0, &[]);
    let (window, shell) = open(cx, empty.clone(), None);
    assert_eq!(setup(cx, &shell), SetupState::Open);
    assert!(shows(cx, window, ROOT_ID));
    assert!(shows(cx, window, STEPS_ID));
    assert!(
        no_tab_selected(cx, window),
        "a tab reads as active under setup"
    );
    assert!(shell.read_with(cx, |s, _| s.graph.is_none()));
    assert_eq!(empty.config_view.calls(), 2);

    let indexed = fixture(3, &[]);
    let (window, shell) = open(cx, indexed.clone(), None);
    assert_eq!(setup(cx, &shell), SetupState::Closed);
    assert!(!shows(cx, window, ROOT_ID));
    assert!(shows(cx, window, "graph"));
    assert_eq!(
        indexed.config_view.calls(),
        0,
        "config was read for an indexed machine"
    );

    let configured = fixture(0, &["/home/u/notes"]);
    let (window, shell) = open(cx, configured.clone(), None);
    assert_eq!(setup(cx, &shell), SetupState::Closed);
    assert!(!shows(cx, window, ROOT_ID));
    assert_eq!(configured.config_view.calls(), 1);

    let dir = tempfile::tempdir().unwrap();
    let path = state_file(&dir);
    app_state::save(
        &path,
        app_state::AppState {
            setup_dismissed: true,
        },
    )
    .unwrap();
    let skipped = fixture(0, &[]);
    let (window, shell) = open(cx, skipped.clone(), Some(&path));
    assert_eq!(setup(cx, &shell), SetupState::Closed);
    assert!(!shows(cx, window, ROOT_ID));
    assert_eq!(skipped.config_view.calls(), 0);

    let swapping = fixture(0, &[]);
    swapping.stats.set(Err(DashboardError::Unavailable(
        "the index is being swapped in".into(),
    )));
    let (window, shell) = open(cx, swapping.clone(), Some(&dir.path().join("none.json")));
    assert_eq!(
        setup(cx, &shell),
        SetupState::Closed,
        "a busy store read as empty"
    );
    assert!(!shows(cx, window, ROOT_ID));

    let unreadable = fixture(0, &[]);
    unreadable.config_view.set(Err(DashboardError::Internal(
        "config.toml unreadable".into(),
    )));
    let (_, shell) = open(cx, unreadable, Some(&dir.path().join("none.json")));
    assert_eq!(setup(cx, &shell), SetupState::Closed);
}

#[gpui_kit::test]
fn setup_shows_loading_until_it_has_decided(cx: &mut TestAppContext) {
    let held = fixture(0, &[]);
    let release = held.stats.hold();
    let (window, shell) = cx.update(|cx| {
        theme::install(cx);
        backend::install(cx, held.clone());
        let (window, shell) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1400.), px(900.)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| Shell::new(Gate::Ok, cx)),
        )
        .unwrap();
        (window.downcast::<Root>().unwrap(), shell)
    });
    assert_eq!(setup(cx, &shell), SetupState::Deciding);
    assert_eq!(
        label(cx, window, "setup-deciding").as_deref(),
        Some(LOADING)
    );
    assert!(shell.read_with(cx, |s, _| s.graph.is_none()));
    assert_eq!(
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.find(TabId::Graph.button_id()).selected()
        })
        .unwrap(),
        Some(true)
    );
    release.send(()).unwrap();
    cx.run_until_parked();
    render(cx, window);
    assert_eq!(setup(cx, &shell), SetupState::Open);
    assert!(!shows(cx, window, "setup-deciding"));
}

#[gpui_kit::test]
fn skip_is_remembered_across_restarts(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = state_file(&dir);
    let backend = fixture(0, &[]);
    let (window, shell) = open(cx, backend.clone(), Some(&path));
    assert_eq!(setup(cx, &shell), SetupState::Open);
    assert_eq!(dismissed_on_disk(&path), None);
    click(cx, window, SKIP_ID);
    assert_eq!(setup(cx, &shell), SetupState::Closed);
    assert!(shows(cx, window, "graph"));
    assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Graph);
    assert_eq!(dismissed_on_disk(&path), Some(true));
    let beside: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(beside, [FILE_NAME]);

    let calls = backend.config_view.calls();
    let (window, shell) = open(cx, backend.clone(), Some(&path));
    assert_eq!(
        setup(cx, &shell),
        SetupState::Closed,
        "the skip was forgotten"
    );
    assert!(!shows(cx, window, ROOT_ID));
    assert_eq!(backend.config_view.calls(), calls);

    std::fs::write(&path, "{ not json").unwrap();
    let (window, shell) = open(cx, backend, Some(&path));
    assert_eq!(setup(cx, &shell), SetupState::Open);
    assert!(shows(cx, window, ROOT_ID));
}

#[gpui_kit::test]
fn clicking_any_tab_closes_setup_without_skipping_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = state_file(&dir);
    for tab in TabId::ALL {
        let (window, shell) = open(cx, fixture(0, &[]), Some(&path));
        assert_eq!(setup(cx, &shell), SetupState::Open);
        click(cx, window, tab.button_id());
        assert_eq!(setup(cx, &shell), SetupState::Closed, "{tab:?}");
        assert_eq!(shell.read_with(cx, |s, _| s.tab()), tab);
        assert!(shows(cx, window, tab.id()), "{tab:?}");
        assert!(!shows(cx, window, ROOT_ID));
    }
    assert_eq!(
        dismissed_on_disk(&path),
        None,
        "a tab click dismissed setup"
    );
    let (_, shell) = open(cx, fixture(0, &[]), Some(&path));
    assert_eq!(setup(cx, &shell), SetupState::Open);
}

#[gpui_kit::test]
fn the_graphs_setup_button_reopens_onboarding_at_the_first_step(cx: &mut TestAppContext) {
    let backend = fixture(0, &[]);
    backend.graph.set(Ok(Graph {
        nodes: Vec::new(),
        entities: Vec::new(),
        tags: Vec::new(),
        edges: Vec::new(),
    }));
    let (window, shell) = open(cx, backend, None);
    click(cx, window, SKIP_ID);
    assert_eq!(setup(cx, &shell), SetupState::Closed);
    click(cx, window, "graph-setup");
    assert_eq!(setup(cx, &shell), SetupState::Open);
    assert!(shows(cx, window, ROOT_ID));
    let view = onboarding(cx, &shell);
    assert_eq!(view.read_with(cx, |v, _| v.step()), 0);
}

fn node(id: &str, title: &str, source_type: &str, chunks: u32) -> Node {
    Node {
        id: id.into(),
        title: title.into(),
        source_type: source_type.into(),
        chunks,
        inbound: 0,
        agent: None,
        memory_kind: None,
        memory_id: None,
    }
}

fn graph() -> Graph {
    Graph {
        nodes: vec![
            node("s", "A long session", "transcript", 99),
            node("m", "A memory", "memory", 80),
            node("p", "A project", "project", 70),
            node("blank", "   ", "markdown", 60),
            node("b", "Beta notes", "markdown", 30),
            node("a", "alpha notes ", "markdown", 30),
            node("small", "Small", "pdf", 5),
        ],
        entities: Vec::new(),
        tags: Vec::new(),
        edges: Vec::new(),
    }
}

#[test]
fn the_suggested_query_is_the_largest_note_and_falls_back_to_a_generic_one() {
    assert_eq!(suggest_query(Some(&graph())), "alpha notes");
    assert_eq!(suggest_query(None), GENERIC_QUERY);
    let only_sessions = Graph {
        nodes: vec![node("s", "A session", "transcript", 9)],
        ..graph()
    };
    assert_eq!(suggest_query(Some(&only_sessions)), GENERIC_QUERY);
    assert!(locale_compare("alpha", "Beta").is_lt());
    assert!(locale_compare("apple", "Apple").is_lt());
    assert!(locale_compare("_draft", "1 note").is_lt());
    assert!(locale_compare("2 notes", "a note").is_lt());
}

#[gpui_kit::test]
fn finishing_seeds_search_with_the_largest_note(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = state_file(&dir);
    let backend = fixture(0, &[]);
    backend.graph.set(Ok(graph()));
    backend.index_start.set(Ok(IndexStarted {
        started: true,
        pid: 7,
        reindex: false,
    }));
    backend.index_status.set(IndexRunView {
        run: Some(IndexRun {
            pid: 7,
            reindex: false,
            finished: true,
            ok: Some(true),
            code: Some(0),
            log: "/tmp/db.log".into(),
        }),
    });
    let (window, shell) = open(cx, backend.clone(), Some(&path));
    let stats_calls = backend.stats.calls();
    click(cx, window, WITHOUT_SAVING_ID);
    assert_eq!(onboarding(cx, &shell).read_with(cx, |v, _| v.step()), 1);
    click(cx, window, CONTINUE_ID);
    cx.run_until_parked();
    render(cx, window);
    assert_eq!(setup(cx, &shell), SetupState::Closed);
    assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Search);
    let search = shell
        .read_with(cx, |s, _| s.search.clone())
        .expect("search opened");
    assert_eq!(
        search.read_with(cx, |s, _| s.query().to_string()),
        "alpha notes"
    );
    assert!(backend
        .requests()
        .iter()
        .any(|r| r.starts_with("search \"alpha notes\"")));
    assert!(shows(cx, window, "suggested-query"));
    assert!(shell.read_with(cx, |s, _| s.graph.is_some()));
    assert_eq!(shell.read_with(cx, |s, _| s.index_runs()), 1);
    assert!(
        backend.stats.calls() > stats_calls,
        "the stats were not refreshed"
    );
    assert_eq!(dismissed_on_disk(&path), Some(true));
    assert_eq!(
        backend
            .requests()
            .iter()
            .filter(|r| r.starts_with("index_start"))
            .count(),
        1
    );
}

#[gpui_kit::test]
fn a_failed_index_offers_try_again_and_never_finishes(cx: &mut TestAppContext) {
    let backend = fixture(0, &[]);
    backend.index_start.set(Ok(IndexStarted {
        started: true,
        pid: 8,
        reindex: false,
    }));
    backend.index_status.set(IndexRunView {
        run: Some(IndexRun {
            pid: 8,
            reindex: false,
            finished: true,
            ok: Some(false),
            code: Some(2),
            log: "/tmp/db.log".into(),
        }),
    });
    let (window, shell) = open(cx, backend.clone(), None);
    click(cx, window, WITHOUT_SAVING_ID);
    click(cx, window, CONTINUE_ID);
    assert_eq!(setup(cx, &shell), SetupState::Open);
    assert!(shows(cx, window, super::TRY_AGAIN_ID));
    assert!(!shows(cx, window, "setup-finishing"));
    assert_eq!(backend.graph.calls(), 0);
    click(cx, window, super::TRY_AGAIN_ID);
    assert_eq!(
        backend
            .requests()
            .iter()
            .filter(|r| r.starts_with("index_start"))
            .count(),
        2
    );
}

#[gpui_kit::test]
fn the_connect_step_highlights_claude_code(cx: &mut TestAppContext) {
    let backend = fixture(0, &[]);
    let (window, shell) = open(cx, backend.clone(), None);
    click(cx, window, WITHOUT_SAVING_ID);
    assert_eq!(
        label(cx, window, part_id("claude-code", "highlight")).as_deref(),
        Some(RECOMMENDED)
    );
    assert_eq!(label(cx, window, part_id("cursor", "highlight")), None);
    assert!(shows(cx, window, card_id("cursor")));
    assert!(
        !shows(cx, window, card_id("codex")),
        "an absent agent got a card"
    );

    let release = backend.agent_connect.hold();
    backend.agent_connect.set(Ok(AgentChanged {
        agent: Value::Null,
        change: Change {
            files: vec![PathBuf::from("/home/u/.cursor/mcp.json")],
            backups: Vec::new(),
            notes: Vec::new(),
        },
    }));
    let view = onboarding(cx, &shell);
    click_now(cx, window, part_id("cursor", "connect"));
    click_now(cx, window, CONTINUE_ID);
    assert_eq!(
        view.read_with(cx, |v, _| v.step()),
        1,
        "Continue went on while busy"
    );
    release.send(()).unwrap();
    cx.run_until_parked();
    render(cx, window);
    assert!(backend
        .requests()
        .iter()
        .any(|r| r.starts_with("agent_connect AgentRequest { id: \"cursor\"")));
    assert_eq!(backend.agents.calls(), 2, "the agent list was not reloaded");
    click(cx, window, CONTINUE_ID);
    assert_eq!(view.read_with(cx, |v, _| v.step()), 2);
}

#[gpui_kit::test]
fn a_folder_is_checked_added_and_saved_before_the_next_step(cx: &mut TestAppContext) {
    let backend = fixture(0, &[]);
    backend
        .config_check
        .set(Ok(rhebok::dashboard::core::config::ConfigChecked {
            errors: Vec::new(),
            blocking: Vec::new(),
        }));
    let (window, shell) = open(cx, backend.clone(), None);
    assert_eq!(
        label(cx, window, "setup-no-folders").as_deref(),
        Some(super::NO_FOLDERS)
    );

    type_text(cx, window, "notes");
    assert_eq!(
        label(cx, window, "setup-problem").as_deref(),
        Some(RELATIVE)
    );
    click(cx, window, ADD_ID);
    let view = onboarding(cx, &shell);
    assert!(view.read_with(cx, |v, _| v.folders().is_empty()));

    type_text(cx, window, "~/notes");
    assert!(!shows(cx, window, "setup-problem"));
    assert!(backend
        .requests()
        .iter()
        .any(|r| r.starts_with("config_check") && r.contains("~/notes")));
    click(cx, window, ADD_ID);
    assert_eq!(view.read_with(cx, |v, _| v.folders().to_vec()), ["~/notes"]);
    assert_eq!(label(cx, window, folder_id(0)).as_deref(), Some("~/notes"));

    backend.config_save.set(Err(DashboardError::ConfigChanged {
        message: "changed".into(),
        etag: "etag-2".into(),
    }));
    backend.config_view.set(Ok(config(&[], "etag-2")));
    click(cx, window, SAVE_ID);
    assert_eq!(
        view.read_with(cx, |v, _| v.notice().map(str::to_string)),
        Some(CONFLICT.into())
    );
    assert_eq!(view.read_with(cx, |v, _| v.step()), 0);
    assert_eq!(view.read_with(cx, |v, _| v.folders().to_vec()), ["~/notes"]);

    backend.config_save.set(Ok(config(&["~/notes"], "etag-3")));
    click(cx, window, SAVE_ID);
    let saves: Vec<String> = backend
        .requests()
        .into_iter()
        .filter(|r| r.starts_with("config_save"))
        .collect();
    assert_eq!(saves.len(), 2);
    assert!(
        saves[1].contains("etag-2") && saves[1].contains("~/notes"),
        "{}",
        saves[1]
    );
    assert_eq!(view.read_with(cx, |v, _| v.step()), 1);
}

#[test]
fn the_tie_break_orders_titles_the_way_javascripts_locale_compare_does() {
    assert_eq!(super::collation_table::NODE_LOCALE, "en-US");
    for (a, b, sign) in [
        ("_inbox", "(draft)", -1),
        ("my-note", "my_note", 1),
        ("Résumé", "Retro", -1),
        ("Über", "Zeitplan", -1),
        ("a—b", "a+b", -1),
    ] {
        assert_eq!(locale_compare(a, b) as i8, sign, "{a:?} vs {b:?}");
    }
    let wrong: Vec<String> = super::collation_table::PAIRS
        .iter()
        .filter(|(a, b, sign)| {
            locale_compare(a, b) as i8 != *sign || locale_compare(b, a) as i8 != -sign
        })
        .map(|(a, b, sign)| {
            format!(
                "{a:?} vs {b:?}: node {sign}, native {}",
                locale_compare(a, b) as i8
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} pairs disagree with node:\n{}",
        wrong.len(),
        super::collation_table::PAIRS.len(),
        wrong.join("\n")
    );
}

#[gpui_kit::test]
fn the_skip_is_on_disk_before_the_click_returns(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = state_file(&dir);
    let (window, shell) = open(cx, fixture(0, &[]), Some(&path));
    click_now(cx, window, SKIP_ID);
    assert_eq!(
        dismissed_on_disk(&path),
        Some(true),
        "the skip waits on a task"
    );
    shell.update(cx, |s, cx| s.finish_setup("q".into(), cx));
    std::fs::remove_file(&path).unwrap();
    shell.update(cx, |s, cx| s.finish_setup("q".into(), cx));
    assert_eq!(
        dismissed_on_disk(&path),
        Some(true),
        "the finish waits on a task"
    );
}

#[gpui_kit::test]
fn a_conflict_reread_checks_the_pending_folder_again(cx: &mut TestAppContext) {
    let backend = fixture(0, &[]);
    backend
        .config_check
        .set(Ok(rhebok::dashboard::core::config::ConfigChecked {
            errors: Vec::new(),
            blocking: Vec::new(),
        }));
    let (window, _) = open(cx, backend.clone(), None);
    type_text(cx, window, "/home/u/kept");
    click(cx, window, ADD_ID);
    type_text(cx, window, "~/notes");
    assert!(!shows(cx, window, "setup-problem"));
    let checks = || {
        backend
            .requests()
            .iter()
            .filter(|r| r.starts_with("config_check") && r.contains("~/notes"))
            .count()
    };
    let before = checks();
    let missing = "sources: ~/notes does not exist";
    backend
        .config_check
        .set(Ok(rhebok::dashboard::core::config::ConfigChecked {
            errors: vec![rhebok::config::check::FieldError::new("sources", missing)],
            blocking: Vec::new(),
        }));
    backend.config_save.set(Err(DashboardError::ConfigChanged {
        message: "changed".into(),
        etag: "etag-2".into(),
    }));
    click(cx, window, SAVE_ID);
    cx.executor().advance_clock(PROBE_DELAY);
    cx.run_until_parked();
    render(cx, window);
    assert_eq!(checks(), before + 1, "the candidate was not checked again");
    assert_eq!(label(cx, window, "setup-problem").as_deref(), Some(missing));
}
