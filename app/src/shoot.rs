use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    assets::Assets, px, size, AnyWindowHandle, AppContext, Bounds, Entity, HeadlessAppContext,
    SharedString, WindowBounds, WindowOptions,
};
use image::{imageops, RgbaImage};

use crate::app_state;
use crate::backend::{self, Answer, Backend, CoreBackend, RetryingReads};
use crate::onboarding::Onboarding;
use crate::shell::{SetupState, Shell, TabId};
use crate::tabs::memories::Leave;
use crate::tabs::search::{card_id, stage, toggle_id, Ran};
use crate::tabs::settings::fields::input_id;
use crate::tabs::settings::nav::Section;
use crate::tabs::settings::raw_toml;
use crate::theme;
use crate::version_gate::{self, Gate};
use crate::widgets::agent_card::CardEvent;

const WIDTH: f32 = 1400.;
const HEIGHT: f32 = 900.;
const SETTLE_LIMIT: Duration = Duration::from_secs(60);
const SETTLE_POLL: Duration = Duration::from_millis(25);

pub const PARITY_WIDTH: &str = "RHEBOK_PARITY_WIDTH";

pub fn window_width() -> f32 {
    std::env::var(PARITY_WIDTH)
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or(WIDTH)
}

pub fn tab_named(name: &str) -> Option<TabId> {
    TabId::ALL.into_iter().find(|tab| tab.id() == name)
}

pub fn settle(cx: &mut HeadlessAppContext) -> Result<(), String> {
    let started = Instant::now();
    cx.allow_parking();
    loop {
        cx.run_until_parked();
        let pending = cx.update(|cx| backend::pending_backend_tasks(cx));
        if pending == 0 {
            cx.run_until_parked();
            return Ok(());
        }
        if started.elapsed() > SETTLE_LIMIT {
            return Err(format!(
                "{pending} backend tasks still pending after {}s",
                SETTLE_LIMIT.as_secs()
            ));
        }
        std::thread::sleep(SETTLE_POLL);
    }
}

pub fn throwaway_home(home: &Path) -> bool {
    let mut parts = home.components().rev();
    let last = parts.next().and_then(|c| c.as_os_str().to_str());
    let parent = parts.next().and_then(|c| c.as_os_str().to_str());
    last == Some("home") && parent.is_some_and(|p| p.starts_with("rhebok-e2e-"))
}

#[derive(Debug, Clone, Default)]
pub struct WritePaths {
    pub home: Option<PathBuf>,
    pub config: PathBuf,
    pub db: PathBuf,
    pub codex_home: Option<PathBuf>,
}

impl WritePaths {
    pub fn from_env() -> WritePaths {
        WritePaths {
            home: std::env::var_os("HOME").map(PathBuf::from),
            config: rhebok::config::Config::config_path(),
            db: rhebok::config::Config::db_path(),
            codex_home: std::env::var_os("CODEX_HOME").map(PathBuf::from),
        }
    }

    fn named(&self) -> Vec<(&'static str, PathBuf)> {
        let mut out = vec![
            ("RHEBOK_CONFIG", self.config.clone()),
            ("the env file", rhebok::env_file::env_path(&self.config)),
            ("RHEBOK_DB", self.db.clone()),
            ("the memory store", rhebok::memory::root(&self.db)),
            ("the app state", app_state::path_beside(&self.db)),
        ];
        if let Some(codex) = &self.codex_home {
            out.push(("CODEX_HOME", codex.clone()));
        }
        out
    }
}

pub fn resolved(path: &Path) -> Option<PathBuf> {
    let escapes = path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    if !path.is_absolute() || escapes {
        return None;
    }
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        match existing.canonicalize() {
            Ok(found) => {
                let mut out = found;
                out.extend(missing.iter().rev());
                return Some(out);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if existing.symlink_metadata().is_ok() {
                    return None;
                }
                missing.push(existing.file_name()?.to_os_string());
                existing = existing.parent()?;
            }
            Err(_) => return None,
        }
    }
}

pub fn inside(path: &Path, root: &Path) -> bool {
    resolved(path).is_some_and(|path| path.starts_with(root))
}

pub const SETTINGS_WRITES: [&str; 6] = [
    "conflict",
    "reindex",
    "notice",
    "saved",
    "test-ok",
    "test-fail",
];

pub fn guard_writes(
    variable: &str,
    state: Option<&str>,
    writes: impl Fn(&str) -> bool,
    what: &str,
    paths: &WritePaths,
) -> Result<(), String> {
    let Some(state) = state.filter(|s| !s.is_empty() && writes(s)) else {
        return Ok(());
    };
    let home = match &paths.home {
        Some(home) if throwaway_home(home) => home,
        Some(home) => {
            return Err(format!(
                "{variable}={state} {what}; refusing because HOME {} is not a throwaway rhebok-e2e sandbox",
                home.display()
            ))
        }
        None => {
            return Err(format!(
                "{variable}={state} {what}; refusing because HOME is not set"
            ))
        }
    };
    let given = home.parent().unwrap_or(home);
    let Ok(root) = given.canonicalize() else {
        return Err(format!(
            "{variable}={state} {what}; refusing because the rhebok-e2e sandbox {} cannot be resolved",
            given.display()
        ));
    };
    for (name, path) in paths.named() {
        if !inside(&path, &root) {
            return Err(format!(
                "{variable}={state} {what}; refusing because {name} {} is outside the rhebok-e2e sandbox {}",
                path.display(),
                root.display()
            ));
        }
    }
    Ok(())
}

pub fn guard_agent_writes(state: Option<&str>, paths: &WritePaths) -> Result<(), String> {
    guard_writes(
        PARITY_INTEGRATIONS,
        state,
        |_| true,
        "writes agent configs",
        paths,
    )
}

pub const ONBOARDING_WRITES: [&str; 1] = ["search"];

pub fn guard_onboarding_writes(state: Option<&str>, paths: &WritePaths) -> Result<(), String> {
    guard_writes(
        PARITY_ONBOARDING,
        state,
        |s| ONBOARDING_WRITES.contains(&s),
        "saves config.toml, indexes and writes app-state.json",
        paths,
    )
}

pub fn guard_settings_writes(state: Option<&str>, paths: &WritePaths) -> Result<(), String> {
    guard_writes(
        PARITY_SETTINGS,
        state,
        |s| SETTINGS_WRITES.contains(&s),
        "saves config.toml or starts an index",
        paths,
    )
}

fn guard_agent_writes_from_env() -> Result<(), String> {
    let state = std::env::var(PARITY_INTEGRATIONS).ok();
    guard_agent_writes(state.as_deref(), &WritePaths::from_env())
}

fn guard_settings_writes_from_env() -> Result<(), String> {
    let state = std::env::var(PARITY_SETTINGS).ok();
    guard_settings_writes(state.as_deref(), &WritePaths::from_env())
}

fn guard_onboarding_writes_from_env() -> Result<(), String> {
    let state = std::env::var(PARITY_ONBOARDING).ok();
    guard_onboarding_writes(state.as_deref(), &WritePaths::from_env())
}

struct Staged {
    core: CoreBackend,
    held_index: bool,
    failed_graph: bool,
    fake_start: bool,
}

impl Backend for Staged {
    fn stats(&self) -> Answer<rhebok::dashboard::core::stats::Stats> {
        self.core.stats()
    }
    fn progress(&self) -> Answer<rhebok::dashboard::core::stats::Progress> {
        if self.held_index {
            Ok(rhebok::dashboard::core::stats::Progress::Idle { idle: true })
        } else {
            self.core.progress()
        }
    }
    fn graph(&self) -> Answer<rhebok::dashboard::core::graph::Graph> {
        if self.failed_graph {
            Err(rhebok::dashboard::core::DashboardError::Unavailable(
                "parity: the graph load fails".to_string(),
            ))
        } else {
            self.core.graph()
        }
    }
    fn document(&self, id: String) -> Answer<rhebok::dashboard::core::document::Document> {
        self.core.document(id)
    }
    fn version(&self, refresh: bool) -> Answer<rhebok::dashboard::core::version::Version> {
        self.core.version(refresh)
    }
    fn search(
        &self,
        query: rhebok::dashboard::core::search::SearchQuery,
    ) -> Answer<rhebok::dashboard::core::search::SearchOutcome> {
        self.core.search(query)
    }
    fn memories(&self) -> rhebok::dashboard::core::memories::Memories {
        self.core.memories()
    }
    fn memory_reach(&self, id: String) -> Answer<rhebok::dashboard::core::memories::MemoryReach> {
        self.core.memory_reach(id)
    }
    fn memory_save(
        &self,
        request: rhebok::dashboard::core::memory_write::SaveRequest,
    ) -> Answer<rhebok::dashboard::core::memory_write::SaveOutcome> {
        self.core.memory_save(request)
    }
    fn memory_delete(
        &self,
        request: rhebok::dashboard::core::memory_write::DeleteRequest,
    ) -> Answer<rhebok::dashboard::core::memory_write::DeleteOutcome> {
        self.core.memory_delete(request)
    }
    fn config_view(&self) -> Answer<rhebok::dashboard::core::config::ConfigView> {
        self.core.config_view()
    }
    fn config_save(
        &self,
        request: rhebok::dashboard::core::config::ConfigSave,
    ) -> Answer<rhebok::dashboard::core::config::ConfigSaved> {
        self.core.config_save(request)
    }
    fn config_check(
        &self,
        request: rhebok::dashboard::core::config::ConfigCheck,
    ) -> Answer<rhebok::dashboard::core::config::ConfigChecked> {
        self.core.config_check(request)
    }
    fn set_embed_endpoint(
        &self,
        request: rhebok::dashboard::core::config::EmbedEndpoint,
    ) -> Answer<rhebok::dashboard::core::config::EmbedEndpointSaved> {
        self.core.set_embed_endpoint(request)
    }
    fn embed_test(&self) -> rhebok::dashboard::core::embed::EmbedTest {
        self.core.embed_test()
    }
    fn ignore_preview(
        &self,
        pattern: String,
    ) -> Answer<rhebok::dashboard::core::ignore::IgnorePreview> {
        self.core.ignore_preview(pattern)
    }
    fn agents(&self) -> rhebok::dashboard::core::agents::AgentList {
        self.core.agents()
    }
    fn agent_connect(
        &self,
        request: rhebok::dashboard::core::agents::AgentRequest,
    ) -> Answer<rhebok::dashboard::core::agents::AgentChanged> {
        self.core.agent_connect(request)
    }
    fn agent_disconnect(
        &self,
        request: rhebok::dashboard::core::agents::AgentRequest,
    ) -> Answer<rhebok::dashboard::core::agents::AgentChanged> {
        self.core.agent_disconnect(request)
    }
    fn index_start(&self, reindex: bool) -> Answer<rhebok::dashboard::core::index::IndexStarted> {
        if self.fake_start {
            Ok(rhebok::dashboard::core::index::IndexStarted {
                started: true,
                pid: PARITY_PID,
                reindex,
            })
        } else {
            self.core.index_start(reindex)
        }
    }
    fn index_status(&self) -> rhebok::dashboard::core::index::IndexRunView {
        if self.held_index {
            rhebok::dashboard::core::index::IndexRunView { run: None }
        } else {
            self.core.index_status()
        }
    }
    fn update_start(&self) -> Answer<rhebok::dashboard::core::update::UpdateStarted> {
        self.core.update_start()
    }
    fn restart(&self) -> Answer<crate::backend::AppRestarted> {
        self.core.restart()
    }
    fn install(&self) -> rhebok::dashboard::core::install::InstallView {
        self.core.install()
    }
}

pub fn gate_refusal(gate: &Gate, show: bool) -> Result<(), String> {
    if *gate == Gate::Ok || show {
        return Ok(());
    }
    Err(format!(
        "version gate refused: {}",
        crate::shell::gate_message(gate)
    ))
}

pub fn render(target: Option<TabId>) -> Result<RgbaImage, String> {
    guard_agent_writes_from_env()?;
    guard_settings_writes_from_env()?;
    guard_onboarding_writes_from_env()?;
    let onboarding = std::env::var(PARITY_ONBOARDING).ok();
    let tab = target.unwrap_or_default();
    let gate = version_gate::check(&version_gate::installed_cli(), rhebok::VERSION);
    gate_refusal(&gate, std::env::var(PARITY_GATE).is_ok())?;
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        theme::install(cx);
        let core = CoreBackend::from_env();
        let held_index =
            tab == TabId::Settings && std::env::var(PARITY_SETTINGS).as_deref() == Ok("reindex");
        let failed_graph =
            target == Some(TabId::Graph) && std::env::var(PARITY_GRAPH).as_deref() == Ok("error");
        let fake_start = target.is_none() && onboarding.as_deref() == Some("index");
        let held_index = held_index || fake_start;
        let backend: Arc<dyn Backend> = if held_index || failed_graph {
            Arc::new(RetryingReads::new(Staged {
                core,
                held_index,
                failed_graph,
                fake_start,
            }))
        } else {
            Arc::new(RetryingReads::new(core))
        };
        backend::install(cx, backend);
        app_state::install(cx, app_state::default_path());
    });
    let (window, shell) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(window_width()), px(HEIGHT)),
                    })),
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                move |_window, cx| cx.new(|cx| Shell::new(gate, cx)),
            )
        })
        .map_err(|e| format!("open window: {e}"))?;
    if let Some(tab) = target {
        cx.update(|cx| shell.update(cx, |shell, cx| shell.open_tab(tab, cx)));
    }
    settle(&mut cx)?;
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .map_err(|e| format!("render: {e}"))?;
    settle(&mut cx)?;
    if tab == TabId::Search {
        if let Ok(query) = std::env::var(PARITY_QUERY) {
            search_for(&mut cx, window, &shell, query)?;
        }
    }
    if tab == TabId::Memories {
        if let Ok(state) = std::env::var(PARITY_MEMORIES) {
            memories_in(&mut cx, &shell, &state)?;
            cx.update_window(window, |_, window, cx| window.render_frame(cx))
                .map_err(|e| format!("render: {e}"))?;
            settle(&mut cx)?;
        }
    }
    if tab == TabId::Integrations {
        if let Ok(state) = std::env::var(PARITY_INTEGRATIONS) {
            integrations_in(&mut cx, window, &shell, &state)?;
        }
    }
    if tab == TabId::Settings {
        if let Ok(state) = std::env::var(PARITY_SETTINGS) {
            settings_in(&mut cx, window, &shell, &state)?;
        }
    }
    if target.is_none() {
        onboarding_in(
            &mut cx,
            window,
            &shell,
            onboarding.as_deref().unwrap_or("folders"),
        )?;
    }
    if target == Some(TabId::Graph) {
        graph_in(
            &mut cx,
            window,
            &shell,
            std::env::var(PARITY_GRAPH).ok().as_deref(),
        )?;
    }
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .map_err(|e| format!("render: {e}"))?;
    cx.capture_screenshot(window)
        .map_err(|e| format!("capture: {e}"))
}

pub const PARITY_QUERY: &str = "RHEBOK_PARITY_QUERY";
pub const PARITY_EXPAND: &str = "RHEBOK_PARITY_EXPAND";
pub const PARITY_MEMORIES: &str = "RHEBOK_PARITY_MEMORIES";
pub const PARITY_INTEGRATIONS: &str = "RHEBOK_PARITY_INTEGRATIONS";
pub const PARITY_SETTINGS: &str = "RHEBOK_PARITY_SETTINGS";
pub const PARITY_GRAPH: &str = "RHEBOK_PARITY_GRAPH";
pub const PARITY_GATE: &str = "RHEBOK_PARITY_GATE";
pub const PARITY_ONBOARDING: &str = "RHEBOK_PARITY_ONBOARDING";
pub const PARITY_FOLDER: &str = "RHEBOK_PARITY_FOLDER";
pub const PARITY_PID: u32 = 1;
pub const PARITY_NODES: &str = "RHEBOK_PARITY_NODES";
pub const PARITY_SEARCH: &str = "how often do I feed the sourdough starter";
pub const PARITY_WHEEL: f64 = -100.;
pub const PARITY_DOCUMENT: &str = "Flour";
pub const PARITY_PANNED_WHEEL: f64 = -500.;

fn wait_until(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    what: &str,
    mut done: impl FnMut(&mut HeadlessAppContext) -> bool,
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        settle(cx)?;
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .map_err(|e| format!("render: {e}"))?;
        if done(cx) {
            return Ok(());
        }
        if started.elapsed() > SETTLE_LIMIT {
            return Err(format!("{what} never happened"));
        }
        std::thread::sleep(SETTLE_POLL);
        cx.advance_clock(SETTLE_POLL);
    }
}

fn settings_in(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    state: &str,
) -> Result<(), String> {
    guard_settings_writes_from_env()?;
    let view = cx
        .update(|cx| shell.read(cx).settings.clone())
        .ok_or("the Settings tab never opened")?;
    let checked = |cx: &mut HeadlessAppContext| {
        cx.update(|cx| {
            let s = view.read(cx);
            !s.pending().is_empty() && !s.checking()
        })
    };
    match state {
        "notice" => {
            cx.update(|cx| {
                view.update(cx, |s, cx| {
                    s.set(
                        "embed.model",
                        Some(toml::Value::String("all-minilm".into())),
                        cx,
                    )
                })
            });
            wait_until(cx, window, "the config check", checked)?;
            cx.update(|cx| view.update(cx, |s, cx| s.open_preview(cx)));
            cx.update(|cx| view.update(cx, |s, cx| s.save(cx)));
            wait_until(cx, window, "the saved notice", |cx| {
                cx.update(|cx| view.read(cx).notice().is_some_and(|n| n.reindex))
            })?;
            cx.update(|cx| view.update(cx, |s, cx| s.pick_section(Section::Sources, cx)));
        }
        "retrieval" | "retrieval-end" | "threshold" | "slider-key" => {
            cx.update(|cx| view.update(cx, |s, cx| s.pick_section(Section::Retrieval, cx)));
            settle(cx)?;
            if state == "slider-key" {
                cx.update_window(window, |_, window, cx| {
                    window.render_frame(cx);
                    let id = SharedString::from("slider-hook.threshold");
                    let track = window.find(id.clone()).bounds();
                    let travel = f32::from(track.size.width) - 2. - 15.;
                    let thumb =
                        gpui_kit::point(px(1. + 7.5 + 0.66 * travel), track.size.height / 2.);
                    window.click_at(id, thumb, cx);
                    window.press("right", cx);
                    window.render_frame(cx);
                })
                .map_err(|e| format!("slider: {e}"))?;
                wait_until(cx, window, "the config check", checked)?;
            }
            if state == "threshold" {
                cx.update(|cx| {
                    view.update(cx, |s, cx| {
                        s.set("hook.threshold", Some(toml::Value::Float(0.42)), cx)
                    })
                });
                wait_until(cx, window, "the config check", checked)?;
                click(cx, window, input_id("hook.threshold"))?;
            }
        }
        "embedding" | "embedding-end" | "remote" | "token" | "saved" | "test-ok" | "test-fail"
        | "model" => {
            cx.update(|cx| view.update(cx, |s, cx| s.pick_section(Section::Embedding, cx)));
            settle(cx)?;
            cx.update_window(window, |_, window, cx| window.render_frame(cx))
                .map_err(|e| format!("render: {e}"))?;
            let typed = |cx: &mut HeadlessAppContext, key: &str, text: &str| {
                click(cx, window, input_id(key))?;
                cx.update_window(window, |_, window, cx| window.input(text, cx))
                    .map_err(|e| format!("type: {e}"))?;
                settle(cx)
            };
            if state == "remote" || state == "saved" {
                cx.update(|cx| {
                    view.update(cx, |s, cx| {
                        s.set_embed_mode(crate::tabs::settings::embedding::Mode::Remote, cx)
                    })
                });
                settle(cx)?;
                cx.update_window(window, |_, window, cx| window.render_frame(cx))
                    .map_err(|e| format!("render: {e}"))?;
                if state == "saved" {
                    typed(cx, "endpoint.url", "http://127.0.0.1:9/v1")?;
                    typed(cx, "endpoint.model", "stub-model")?;
                }
                click(cx, window, "token-replace".into())?;
                settle(cx)?;
                cx.update_window(window, |_, window, cx| window.render_frame(cx))
                    .map_err(|e| format!("render: {e}"))?;
                typed(cx, "embed.token", "sk-parity-secret")?;
                if state == "saved" {
                    cx.update(|cx| view.update(cx, |s, cx| s.save_endpoint(cx)));
                    wait_until(cx, window, "the endpoint save", |cx| {
                        cx.update(|cx| view.read(cx).embed_local().saved.is_some())
                    })?;
                }
            }
            if state == "test-ok" || state == "test-fail" {
                cx.update(|cx| view.update(cx, |s, cx| s.test_embedding(cx)));
                wait_until(cx, window, "the connection test", |cx| {
                    cx.update(|cx| view.read(cx).embed_local().test.is_some())
                })?;
            }
            if state == "model" {
                cx.update(|cx| {
                    view.update(cx, |s, cx| {
                        s.set(
                            "embed.model",
                            Some(toml::Value::String("nomic-embed-text".into())),
                            cx,
                        )
                    })
                });
                wait_until(cx, window, "the config check", checked)?;
                click(cx, window, input_id("embed.model"))?;
            }
        }
        "advanced" | "weights" | "raw" | "raw-edit" | "raw-error" | "raw-copied" => {
            cx.update(|cx| view.update(cx, |s, cx| s.pick_section(Section::Advanced, cx)));
            settle(cx)?;
            cx.update_window(window, |_, window, cx| window.render_frame(cx))
                .map_err(|e| format!("render: {e}"))?;
            if state == "weights" {
                click(cx, window, input_id("hook.weights.transcript"))?;
                cx.update_window(window, |_, window, cx| window.input("0.5", cx))
                    .map_err(|e| format!("type: {e}"))?;
                wait_until(cx, window, "the config check", checked)?;
            }
            if state.starts_with("raw") {
                let bottom = |cx: &mut HeadlessAppContext| {
                    cx.update(|cx| view.read(cx).scroll_handle().scroll_to_bottom());
                    cx.update_window(window, |_, window, cx| window.render_frame(cx))
                        .map_err(|e| format!("render: {e}"))
                };
                bottom(cx)?;
                let lines: &[&str] = match state {
                    "raw-edit" => &["[mcp]", "max_tokens = 900", ""],
                    "raw-error" => &["oops = = 1", ""],
                    _ => &[],
                };
                if !lines.is_empty() {
                    click(cx, window, raw_toml::AREA_ID.into())?;
                    cx.update_window(window, |_, window, cx| {
                        window.press("cmd-down", cx);
                        for (ix, line) in lines.iter().enumerate() {
                            if ix > 0 {
                                window.press("enter", cx);
                            }
                            window.input(line, cx);
                        }
                    })
                    .map_err(|e| format!("type: {e}"))?;
                    settle(cx)?;
                }
                if state == "raw-error" {
                    click(cx, window, "raw-apply".into())?;
                }
                if state == "raw-copied" {
                    click(cx, window, "raw-copy".into())?;
                }
                cx.update_window(window, |_, window, cx| window.blur(cx))
                    .map_err(|e| format!("blur: {e}"))?;
                settle(cx)?;
                bottom(cx)?;
            }
        }
        "memory" => cx.update(|cx| view.update(cx, |s, cx| s.pick_section(Section::Memory, cx))),
        "documents" => {
            cx.update(|cx| view.update(cx, |s, cx| s.pick_section(Section::Documents, cx)))
        }
        "updates" | "updates-end" => {
            cx.update(|cx| view.update(cx, |s, cx| s.pick_section(Section::Updates, cx)))
        }
        "narrow" => {
            cx.update(|cx| {
                view.update(cx, |s, cx| {
                    s.set("index_transcripts", Some(toml::Value::Boolean(true)), cx)
                })
            });
            wait_until(cx, window, "the config check", checked)?;
            cx.update(|cx| view.read(cx).scroll_handle().scroll_to_bottom());
        }
        "edit" | "conflict" => {
            cx.update(|cx| {
                view.update(cx, |s, cx| {
                    s.set("index_transcripts", Some(toml::Value::Boolean(true)), cx)
                })
            });
            wait_until(cx, window, "the config check", checked)?;
            if state == "conflict" {
                let config = rhebok::config::Config::config_path();
                let mut text = std::fs::read_to_string(&config)
                    .map_err(|e| format!("{}: {e}", config.display()))?;
                text.push_str("\n# edited by hand\n");
                std::fs::write(&config, text).map_err(|e| format!("{}: {e}", config.display()))?;
            }
            cx.update(|cx| view.update(cx, |s, cx| s.open_preview(cx)));
            if state == "conflict" {
                cx.update(|cx| view.update(cx, |s, cx| s.save(cx)));
                wait_until(cx, window, "the conflict", |cx| {
                    cx.update(|cx| view.read(cx).conflict())
                })?;
            }
        }
        "blocking" | "ring" => {
            click(cx, window, input_id("index_transcripts_max_age_days"))?;
            cx.update_window(window, |_, window, cx| window.input("abc", cx))
                .map_err(|e| format!("type: {e}"))?;
            wait_until(cx, window, "the config check", checked)?;
            if state == "ring" {
                cx.update(|cx| {
                    view.update(cx, |s, _| s.scroll_handle().set_offset(Default::default()))
                });
                cx.update_window(window, |_, window, cx| window.blur(cx))
                    .map_err(|e| format!("blur: {e}"))?;
                cx.update(|cx| {
                    shell.update(cx, |s, cx| {
                        s.open_setting("index_transcripts_max_age_days", cx)
                    })
                });
                wait_until(cx, window, "the ring", |cx| {
                    cx.update(|cx| view.read(cx).ring().is_some())
                })?;
            }
        }
        "reindex" => {
            let core = CoreBackend::from_env();
            wait_until(cx, window, "the previous index to end", |_| {
                core.index_status().run.is_none_or(|run| run.finished)
            })?;
            click(cx, window, "settings-reindex".into())?;
            wait_until(cx, window, "the index to start", |cx| {
                cx.update(|cx| {
                    matches!(
                        view.read(cx).index_status().read(cx).state(),
                        crate::tabs::health::index_panel::IndexState::Running { .. }
                    )
                })
            })?;
            cx.update(|cx| view.read(cx).scroll_handle().scroll_to_bottom());
        }
        _ => {}
    }
    settle(cx)?;
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .map_err(|e| format!("render: {e}"))?;
    if state.ends_with("-end") {
        cx.update(|cx| view.read(cx).scroll_handle().scroll_to_bottom());
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .map_err(|e| format!("render: {e}"))?;
    }
    settle(cx)
}

fn onboarding_in(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    state: &str,
) -> Result<(), String> {
    guard_onboarding_writes_from_env()?;
    wait_until(cx, window, "onboarding to open", |cx| {
        cx.update(|cx| shell.read(cx).setup() != SetupState::Deciding)
    })?;
    let view: Entity<Onboarding> = cx
        .update(|cx| shell.read(cx).onboarding())
        .ok_or("onboarding did not open: this machine has documents, sources or a skip")?;
    let on = |cx: &mut HeadlessAppContext,
              f: &dyn Fn(&mut Onboarding, &mut gpui_kit::Context<Onboarding>)| {
        cx.update(|cx| view.update(cx, |v, cx| f(v, cx)))
    };
    wait_until(cx, window, "config.toml to be read", |cx| {
        cx.update(|cx| view.read(cx).config_read())
    })?;
    let folder = || std::env::var(PARITY_FOLDER).map_err(|_| format!("{PARITY_FOLDER} is not set"));
    if state == "added" || state == "search" {
        let folder = folder()?;
        let typed = folder.clone();
        on(cx, &move |v, cx| v.set_candidate(typed.clone(), cx));
        wait_until(cx, window, "the folder check", |cx| {
            cx.update(|cx| view.read(cx).candidate_checked())
        })?;
        on(cx, &|v, cx| v.add_folder(cx));
        if !cx.update(|cx| view.read(cx).folders().contains(&folder)) {
            return Err(format!("{folder} was not added"));
        }
    }
    if state == "connect" || state == "index" {
        on(cx, &|v, cx| v.go_to(1, cx));
    }
    if state == "search" {
        on(cx, &|v, cx| v.save(cx));
    }
    if state == "connect" || state == "index" || state == "search" {
        wait_until(cx, window, "the agent list", |cx| {
            cx.update(|cx| view.read(cx).step() == 1 && view.read(cx).agents().is_some())
        })?;
    }
    if state == "index" {
        on(cx, &|v, cx| v.continue_from_agents(cx));
        wait_until(cx, window, "the index to start", |cx| {
            cx.update(|cx| {
                view.read(cx).index_status().is_some_and(|s| {
                    matches!(
                        s.read(cx).state(),
                        crate::tabs::health::index_panel::IndexState::Running { .. }
                    )
                })
            })
        })?;
    }
    if state == "search" {
        on(cx, &|v, cx| v.continue_from_agents(cx));
        wait_until(cx, window, "setup to finish on Search", |cx| {
            cx.update(|cx| {
                let shell = shell.read(cx);
                shell.setup() == SetupState::Closed
                    && shell.tab() == TabId::Search
                    && shell
                        .search
                        .as_ref()
                        .is_some_and(|s| s.read(cx).ran().is_some())
            })
        })?;
    }
    settle(cx)?;
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .map_err(|e| format!("render: {e}"))?;
    settle(cx)?;
    if let Ok(path) = std::env::var(PARITY_NODES) {
        let boxes = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let mut boxes = serde_json::Map::new();
                for snapshot in gpui_kit::test::TestQueryExt::elements(window) {
                    if let Some(gpui_kit::ElementId::Name(name)) = snapshot.path().last() {
                        if name.starts_with("setup") || name.starts_with("agent-") {
                            let b = snapshot.bounds();
                            boxes.insert(
                                name.to_string(),
                                serde_json::json!([
                                    f32::from(b.origin.x),
                                    f32::from(b.origin.y),
                                    f32::from(b.size.width),
                                    f32::from(b.size.height)
                                ]),
                            );
                        }
                    }
                }
                boxes
            })
            .map_err(|e| format!("render: {e}"))?;
        std::fs::write(&path, serde_json::Value::Object(boxes).to_string())
            .map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

fn integrations_in(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    state: &str,
) -> Result<(), String> {
    guard_agent_writes_from_env()?;
    let view = cx
        .update(|cx| shell.read(cx).integrations.clone())
        .ok_or("the Integrations tab never opened")?;
    let event = match state {
        "open" => {
            cx.update(|cx| view.update(cx, |v, cx| v.toggle_missing(cx)));
            None
        }
        "confirm" => Some(("cursor", CardEvent::AskDisconnect)),
        "change" => Some(("cursor", CardEvent::Connect)),
        "refusal" => Some(("gemini", CardEvent::Connect)),
        _ => None,
    };
    if let Some((id, event)) = event {
        cx.update(|cx| view.update(cx, |v, cx| v.card_event(id, event, cx)));
    }
    if state == "disconnect" {
        cx.update(|cx| {
            view.update(cx, |v, cx| {
                v.card_event("cursor", CardEvent::AskDisconnect, cx);
                v.card_event("cursor", CardEvent::Disconnect, cx);
            })
        });
    }
    settle(cx)?;
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .map_err(|e| format!("render: {e}"))?;
    if state == "snippets" {
        cx.update(|cx| view.read(cx).scroll_handle().scroll_to_bottom());
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .map_err(|e| format!("render: {e}"))?;
    }
    settle(cx)
}

fn memories_in(
    cx: &mut HeadlessAppContext,
    shell: &Entity<Shell>,
    state: &str,
) -> Result<(), String> {
    let memories = cx
        .update(|cx| shell.read(cx).memories.clone())
        .ok_or("the Memories tab never opened")?;
    cx.update(|cx| {
        memories.update(cx, |m, cx| {
            let first = m.shown_ids().into_iter().next();
            match (state, first) {
                ("open" | "confirm" | "reach" | "long", Some(id)) => {
                    m.request(Leave::Pick(id), cx);
                    match state {
                        "confirm" => m.press_delete(cx),
                        "reach" => m.load_reach(cx),
                        _ => {}
                    }
                }
                ("new", _) => m.request(Leave::New, cx),
                _ => {}
            }
        })
    });
    settle(cx)
}

pub const PARITY_PAN: &str = "RHEBOK_PARITY_PAN";

pub fn web_pan() -> Option<(f64, f64)> {
    let pan = std::env::var(PARITY_PAN).ok()?;
    let (dx, dy) = pan.split_once(',')?;
    Some((dx.trim().parse().ok()?, dy.trim().parse().ok()?))
}

pub fn graph_kind_for(state: &str) -> Option<&'static str> {
    match state {
        "document" => Some("markdown"),
        "project" => Some("project"),
        "memory" => Some("memory"),
        _ => None,
    }
}

fn graph_in(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    state: Option<&str>,
) -> Result<(), String> {
    let graph = cx
        .update(|cx| shell.read(cx).graph.clone())
        .ok_or("the Graph tab never opened")?;
    let render = |cx: &mut HeadlessAppContext| {
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .map_err(|e| format!("render: {e}"))
    };
    if state == Some("ring") {
        cx.update(|cx| shell.update(cx, |s, cx| s.open_tab(TabId::Search, cx)));
        settle(cx)?;
        render(cx)?;
        search_for(cx, window, shell, PARITY_SEARCH.to_string())?;
        cx.update(|cx| shell.update(cx, |s, cx| s.open_tab(TabId::Graph, cx)));
        settle(cx)?;
    }
    if state == Some("sessions") {
        cx.update(|cx| graph.update(cx, |g, cx| g.toggle_sessions(cx)));
    }
    cx.update(|cx| graph.update(cx, |g, _| g.settle()));
    render(cx)?;
    settle(cx)?;
    if let Some(kind) = state.and_then(graph_kind_for) {
        let at = cx.update(|cx| {
            let g = graph.read(cx);
            let nodes = g.shown().nodes.clone();
            nodes
                .iter()
                .position(|n| n.kind == kind && n.title == PARITY_DOCUMENT)
                .or_else(|| nodes.iter().position(|n| n.kind == kind))
                .and_then(|ix| g.screen_of(ix))
        });
        let at = at.ok_or_else(|| format!("no {kind} node to click"))?;
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click_at(
                "graph-canvas",
                gpui_kit::point(px(at.x as f32), px(at.y as f32)),
                cx,
            );
            window.render_frame(cx);
        })
        .map_err(|e| format!("click: {e}"))?;
        settle(cx)?;
        render(cx)?;
        settle(cx)?;
    }
    if state == Some("zoom") {
        cx.update(|cx| {
            graph.update(cx, |g, cx| {
                let size = g.canvas_size();
                let centre =
                    gpui_kit::point(f64::from(size.width) / 2., f64::from(size.height) / 2.);
                g.wheel(centre, PARITY_WHEEL, false, cx);
            })
        });
    }
    if state == Some("panned") {
        cx.update(|cx| {
            graph.update(cx, |g, cx| {
                let size = g.canvas_size();
                let centre =
                    gpui_kit::point(f64::from(size.width) / 2., f64::from(size.height) / 2.);
                g.wheel(centre, PARITY_PANNED_WHEEL, false, cx);
                let corner = (0..g.shown().nodes.len())
                    .filter_map(|ix| g.screen_of(ix))
                    .fold(None::<gpui_kit::Point<f64>>, |a, b| match a {
                        Some(a) if a.x + a.y >= b.x + b.y => Some(a),
                        _ => Some(b),
                    })
                    .unwrap_or_default();
                let (dx, dy) = web_pan().unwrap_or((
                    (f64::from(size.width) - corner.x).round(),
                    (f64::from(size.height) - corner.y).round(),
                ));
                let to = gpui_kit::point(8. + dx, 8. + dy);
                g.press(gpui_kit::point(8., 8.), cx);
                g.moved(to, true, cx);
                g.release(to, cx);
            })
        });
    }
    if state == Some("hover") {
        let at = cx.update(|cx| {
            let g = graph.read(cx);
            g.shown()
                .nodes
                .iter()
                .position(|n| n.kind == "markdown")
                .and_then(|ix| g.screen_of(ix))
        });
        let at = at.ok_or("no markdown node to hover")?;
        cx.update(|cx| graph.update(cx, |g, cx| g.moved(at, true, cx)));
    } else {
        cx.update(|cx| graph.update(cx, |g, cx| g.moved(gpui_kit::point(-1., -1.), false, cx)));
    }
    render(cx)?;
    render(cx)?;
    let tooltip = cx
        .update_window(window, |_, window, _| {
            window.try_find("graph-tooltip").map(|tip| {
                let b = tip.bounds();
                serde_json::json!({
                    "left": f32::from(b.origin.x),
                    "top": f32::from(b.origin.y),
                    "width": f32::from(b.size.width),
                    "height": f32::from(b.size.height),
                    "text": tip.label().unwrap_or_default().to_string(),
                })
            })
        })
        .map_err(|e| format!("render: {e}"))?;
    if let Ok(path) = std::env::var(PARITY_NODES) {
        let nodes = cx.update(|cx| {
            let g = graph.read(cx);
            let view = g.view();
            let nodes: Vec<serde_json::Value> = g
                .shown()
                .nodes
                .iter()
                .enumerate()
                .filter_map(|(ix, n)| {
                    let s = g.screen_of(ix)?;
                    Some(serde_json::json!({ "id": n.id, "kind": n.kind, "sx": s.x, "sy": s.y }))
                })
                .collect();
            serde_json::json!({ "k": view.k, "ticks": g.ticks(), "nodes": nodes, "tooltip": tooltip })
        });
        std::fs::write(&path, nodes.to_string()).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

fn click(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    id: SharedString,
) -> Result<(), String> {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .map_err(|e| format!("click: {e}"))
}

fn search_for(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    query: String,
) -> Result<(), String> {
    let search = cx
        .update(|cx| shell.read(cx).search.clone())
        .ok_or("the Search tab never opened")?;
    click(cx, window, "search-input".into())?;
    cx.update(|cx| search.update(cx, |search, cx| search.submit(query, cx)));
    settle(cx)?;
    if std::env::var(PARITY_EXPAND).is_err() {
        return Ok(());
    }
    let first = cx.update(|cx| match search.read(cx).ran() {
        Some(Ran::Done(ex)) => stage(ex, "vector")
            .first()
            .map(|hit| toggle_id(&card_id("vector", 0, &hit.chunk_id))),
        _ => None,
    });
    let first = first.ok_or("the search returned no vector hit to expand")?;
    click(cx, window, first)?;
    settle(cx)?;
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .map_err(|e| format!("render: {e}"))?;
    settle(cx)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.;
    }
    let ix = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[ix]
}

pub fn measure_graph(sessions: bool) -> Result<(), String> {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        theme::install(cx);
        backend::install(cx, Arc::new(RetryingReads::new(CoreBackend::from_env())));
    });
    let (window, shell) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(WIDTH), px(HEIGHT)),
                    })),
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                move |_window, cx| cx.new(|cx| Shell::new(Gate::Ok, cx)),
            )
        })
        .map_err(|e| format!("open window: {e}"))?;
    settle(&mut cx)?;
    let graph = cx
        .update(|cx| shell.read(cx).graph.clone())
        .ok_or("the Graph tab never opened")?;
    if let Some(error) = cx.update(|cx| graph.read(cx).error().map(str::to_string)) {
        return Err(format!("the graph did not load: {error}"));
    }
    if sessions {
        cx.update(|cx| graph.update(cx, |g, cx| g.toggle_sessions(cx)));
    }
    let (nodes, edges) = cx.update(|cx| {
        let shown = graph.read(cx).shown();
        (shown.nodes.len(), shown.edges.len())
    });
    let mut frames = Vec::new();
    let mut ticks = Vec::new();
    let started = Instant::now();
    loop {
        let frame = Instant::now();
        let warm = cx.update(|cx| graph.update(cx, |g, _| g.tick()));
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .map_err(|e| format!("render: {e}"))?;
        frames.push(frame.elapsed().as_secs_f64() * 1000.);
        if let Some(t) = cx.update(|cx| graph.read(cx).tick_times().last().copied()) {
            ticks.push(t.as_secs_f64() * 1000.);
        }
        if !warm {
            break;
        }
    }
    let total = started.elapsed();
    frames.sort_by(f64::total_cmp);
    ticks.sort_by(f64::total_cmp);
    println!(
        "graph: {nodes} nodes, {edges} edges; {} ticks to cool ({:.1} s at 60 fps), measured in {:.2} s",
        frames.len(),
        frames.len() as f64 / 60.,
        total.as_secs_f64()
    );
    println!(
        "frame (tick + render): p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        percentile(&frames, 0.5),
        percentile(&frames, 0.95),
        percentile(&frames, 1.)
    );
    println!(
        "tick alone: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        percentile(&ticks, 0.5),
        percentile(&ticks, 0.95),
        percentile(&ticks, 1.)
    );
    Ok(())
}

pub fn side_by_side(left: &Path, right: &Path, out: &Path) -> Result<(), String> {
    let open = |p: &Path| {
        image::open(p)
            .map(|i| i.to_rgba8())
            .map_err(|e| format!("{}: {e}", p.display()))
    };
    let (a, b) = (open(left)?, open(right)?);
    let gap = 8;
    let mut canvas = RgbaImage::from_pixel(
        a.width() + gap + b.width(),
        a.height().max(b.height()),
        image::Rgba([128, 128, 128, 255]),
    );
    imageops::overlay(&mut canvas, &a, 0, 0);
    imageops::overlay(&mut canvas, &b, i64::from(a.width() + gap), 0);
    canvas
        .save(out)
        .map_err(|e| format!("{}: {e}", out.display()))
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args {
        [flag, tab, out] if flag == "--screenshot" => {
            let target = match tab.as_str() {
                "onboarding" => None,
                name => Some(tab_named(name).ok_or_else(|| format!("unknown tab {name}"))?),
            };
            let image = render(target)?;
            println!("{}x{}", image.width(), image.height());
            image.save(out).map_err(|e| format!("{out}: {e}"))
        }
        [flag] if flag == "--measure-graph" => measure_graph(false),
        [flag, sessions] if flag == "--measure-graph" && sessions == "sessions" => {
            measure_graph(true)
        }
        [flag, left, right, out] if flag == "--side-by-side" => {
            side_by_side(Path::new(left), Path::new(right), Path::new(out))
        }
        _ => Err("usage: rhebok-app --screenshot <tab|onboarding> <out.png> | --side-by-side <a.png> <b.png> <out.png> | --measure-graph [sessions]".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::version_gate::Gate;

    use super::{
        guard_agent_writes, guard_onboarding_writes, guard_settings_writes, throwaway_home,
        WritePaths,
    };

    struct Sandbox {
        root: tempfile::TempDir,
    }

    impl Sandbox {
        fn new() -> Sandbox {
            let root = tempfile::Builder::new()
                .prefix("rhebok-e2e-")
                .tempdir()
                .unwrap();
            std::fs::create_dir_all(root.path().join("home")).unwrap();
            Sandbox { root }
        }

        fn path(&self, rest: &str) -> PathBuf {
            self.root.path().join(rest)
        }

        fn paths(&self) -> WritePaths {
            WritePaths {
                home: Some(self.path("home")),
                config: self.path("home/.config/rhebok/config.toml"),
                db: self.path("home/.local/share/rhebok/db"),
                codex_home: Some(self.path("home/.codex")),
            }
        }
    }

    #[test]
    fn a_mismatched_gate_is_refused_unless_the_shot_asks_to_show_it() {
        let mismatch = Gate::Mismatch {
            cli: "0.2.3".into(),
            app: "0.2.2".into(),
        };
        let refused = super::gate_refusal(&mismatch, false).unwrap_err();
        assert!(refused.contains("run rhebok install"), "{refused}");
        assert!(super::gate_refusal(&mismatch, true).is_ok());
        assert!(super::gate_refusal(&Gate::Ok, false).is_ok());
    }

    #[test]
    fn agent_writes_refuse_any_home_but_a_throwaway_sandbox() {
        let sandbox = Sandbox::new();
        let sandboxed = || sandbox.paths();
        assert!(throwaway_home(&sandbox.path("home")));
        let root = sandbox.path("");
        let root = root.to_str().unwrap().trim_end_matches('/').to_string();
        for real in [
            "/Users/someone",
            root.as_str(),
            "/tmp/rhebok-e2e-x/home/sub",
            "/tmp/other/home",
        ] {
            assert!(!throwaway_home(Path::new(real)), "{real}");
            let paths = WritePaths {
                home: Some(PathBuf::from(real)),
                ..sandboxed()
            };
            assert!(guard_agent_writes(Some("disconnect"), &paths).is_err());
        }
        let homeless = WritePaths {
            home: None,
            ..sandboxed()
        };
        assert!(guard_agent_writes(Some("change"), &homeless).is_err());
        assert!(guard_agent_writes(Some("disconnect"), &sandboxed()).is_ok());
        let real = WritePaths {
            home: Some(PathBuf::from("/Users/someone")),
            ..sandboxed()
        };
        assert!(guard_agent_writes(None, &real).is_ok());
        assert!(guard_agent_writes(Some(""), &real).is_ok());
    }

    #[test]
    fn settings_writes_refuse_any_home_but_a_throwaway_sandbox() {
        let sandbox = Sandbox::new();
        let sandboxed = || sandbox.paths();
        let real = WritePaths {
            home: Some(PathBuf::from("/Users/someone")),
            ..sandboxed()
        };
        for state in [
            "conflict",
            "reindex",
            "notice",
            "saved",
            "test-ok",
            "test-fail",
        ] {
            let refused = guard_settings_writes(Some(state), &real).unwrap_err();
            assert!(
                refused.contains("not a throwaway rhebok-e2e sandbox"),
                "{refused}"
            );
            assert!(
                guard_settings_writes(Some(state), &sandboxed()).is_ok(),
                "{:?}",
                guard_settings_writes(Some(state), &sandboxed())
            );
        }
        for state in [
            "edit",
            "blocking",
            "ring",
            "",
            "remote",
            "embedding",
            "threshold",
        ] {
            assert!(guard_settings_writes(Some(state), &real).is_ok(), "{state}");
        }
    }

    #[test]
    fn a_write_refuses_any_store_outside_the_sandbox_whatever_home_says() {
        let sandbox = Sandbox::new();
        let sandboxed = || sandbox.paths();
        let escape = sandbox.path("../elsewhere/db");
        let outside = [
            WritePaths {
                config: PathBuf::from(
                    "/Users/someone/Library/Application Support/rhebok/config.toml",
                ),
                ..sandboxed()
            },
            WritePaths {
                db: PathBuf::from("/Users/someone/Library/Application Support/rhebok/db"),
                ..sandboxed()
            },
            WritePaths {
                db: escape.clone(),
                ..sandboxed()
            },
            WritePaths {
                codex_home: Some(PathBuf::from("/Users/someone/.codex")),
                ..sandboxed()
            },
            WritePaths {
                config: PathBuf::from("config.toml"),
                ..sandboxed()
            },
        ];
        for paths in outside {
            for refused in [
                guard_settings_writes(Some("conflict"), &paths),
                guard_settings_writes(Some("reindex"), &paths),
                guard_agent_writes(Some("change"), &paths),
            ] {
                let refused = refused.unwrap_err();
                assert!(
                    refused.contains("outside the rhebok-e2e sandbox"),
                    "{refused}"
                );
            }
        }
    }

    #[test]
    fn a_write_refuses_a_sandbox_path_that_is_a_symlink_out_of_the_sandbox() {
        let sandbox = Sandbox::new();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), sandbox.path("data")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), sandbox.path("home/.codex")).unwrap();
        let linked_db = WritePaths {
            db: sandbox.path("data/rhebok/db"),
            ..sandbox.paths()
        };
        let linked_codex = sandbox.paths();
        for paths in [linked_db, linked_codex] {
            for refused in [
                guard_settings_writes(Some("conflict"), &paths),
                guard_settings_writes(Some("reindex"), &paths),
                guard_agent_writes(Some("change"), &paths),
            ] {
                let refused = refused.unwrap_err();
                assert!(
                    refused.contains("outside the rhebok-e2e sandbox"),
                    "{refused}"
                );
            }
        }
        let fresh = WritePaths {
            codex_home: Some(sandbox.path("home/codex-not-yet")),
            ..sandbox.paths()
        };
        assert!(guard_settings_writes(Some("reindex"), &fresh).is_ok());

        let linked_home = tempfile::Builder::new()
            .prefix("rhebok-e2e-")
            .tempdir()
            .unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), linked_home.path().join("home")).unwrap();
        let home = linked_home.path().join("home");
        let through_home = WritePaths {
            home: Some(home.clone()),
            config: home.join("config.toml"),
            db: linked_home.path().join("db"),
            codex_home: None,
        };
        assert!(guard_settings_writes(Some("conflict"), &through_home).is_err());
    }

    #[test]
    fn an_endpoint_save_refuses_an_env_file_that_links_out_of_the_sandbox() {
        let sandbox = Sandbox::new();
        let elsewhere = tempfile::tempdir().unwrap();
        let config_dir = sandbox.path("home/.config/rhebok");
        std::fs::create_dir_all(&config_dir).unwrap();
        assert!(guard_settings_writes(Some("saved"), &sandbox.paths()).is_ok());
        std::os::unix::fs::symlink(elsewhere.path().join("env"), config_dir.join("env")).unwrap();
        for state in ["saved", "test-ok", "test-fail"] {
            let refused = guard_settings_writes(Some(state), &sandbox.paths()).unwrap_err();
            assert!(refused.contains("the env file"), "{refused}");
        }
        std::fs::write(elsewhere.path().join("env"), "").unwrap();
        let refused = guard_settings_writes(Some("saved"), &sandbox.paths()).unwrap_err();
        assert!(refused.contains("the env file"), "{refused}");
    }

    #[test]
    fn a_write_refuses_a_dangling_symlink_that_points_out_of_the_sandbox() {
        let sandbox = Sandbox::new();
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("not-yet");
        std::os::unix::fs::symlink(&target, sandbox.path("data")).unwrap();
        let paths = WritePaths {
            db: sandbox.path("data/rhebok/db"),
            ..sandbox.paths()
        };
        for refused in [
            guard_settings_writes(Some("reindex"), &paths),
            guard_agent_writes(Some("change"), &paths),
        ] {
            let refused = refused.unwrap_err();
            assert!(
                refused.contains("outside the rhebok-e2e sandbox"),
                "{refused}"
            );
        }
        std::os::unix::fs::symlink(&target, sandbox.path("home/linked-config.toml")).unwrap();
        let config = WritePaths {
            config: sandbox.path("home/linked-config.toml"),
            ..sandbox.paths()
        };
        assert!(guard_settings_writes(Some("conflict"), &config).is_err());
        assert!(!target.exists());
    }

    #[test]
    fn onboarding_writes_refuse_any_home_but_a_throwaway_sandbox() {
        let sandbox = Sandbox::new();
        let real = WritePaths {
            home: Some(PathBuf::from("/Users/someone")),
            ..sandbox.paths()
        };
        let refused = guard_onboarding_writes(Some("search"), &real).unwrap_err();
        assert!(
            refused.contains("not a throwaway rhebok-e2e sandbox"),
            "{refused}"
        );
        assert!(guard_onboarding_writes(Some("search"), &sandbox.paths()).is_ok());
        for state in ["folders", "added", "connect", "index", ""] {
            assert!(
                guard_onboarding_writes(Some(state), &real).is_ok(),
                "{state}"
            );
        }
        assert!(guard_onboarding_writes(None, &real).is_ok());
    }

    #[test]
    fn a_write_refuses_an_app_state_file_that_links_out_of_the_sandbox() {
        let sandbox = Sandbox::new();
        let elsewhere = tempfile::tempdir().unwrap();
        let data = sandbox.path("home/.local/share/rhebok");
        std::fs::create_dir_all(&data).unwrap();
        assert!(guard_onboarding_writes(Some("search"), &sandbox.paths()).is_ok());
        std::os::unix::fs::symlink(
            elsewhere.path().join("app-state.json"),
            data.join("app-state.json"),
        )
        .unwrap();
        for refused in [
            guard_onboarding_writes(Some("search"), &sandbox.paths()),
            guard_settings_writes(Some("conflict"), &sandbox.paths()),
            guard_agent_writes(Some("change"), &sandbox.paths()),
        ] {
            let refused = refused.unwrap_err();
            assert!(refused.contains("the app state"), "{refused}");
        }
        assert!(!elsewhere.path().join("app-state.json").exists());
    }
}
