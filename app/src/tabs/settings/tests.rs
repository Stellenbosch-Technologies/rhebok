use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, size, AppContext, Bounds, Entity, Point, SharedString, TestAppContext, WindowBounds,
    WindowHandle, WindowOptions,
};
use rhebok::config::check::FieldError;
use rhebok::dashboard::core::config::{ConfigChecked, ConfigView};
use rhebok::dashboard::core::index::{IndexRun, IndexRunView, IndexStarted};
use rhebok::dashboard::core::stats::{Stats, Tiers};
use rhebok::dashboard::core::DashboardError;
use toml::Value;

use super::nav::{field_id, Section};
use super::Settings;
use crate::backend::{self, FixtureBackend};
use crate::shell::{Shell, TabId};
use crate::theme;
use crate::version_gate::Gate;

pub const CHECK_DEBOUNCE: Duration = Duration::from_millis(300);

pub(super) fn stats() -> Stats {
    Stats {
        documents: 4,
        chunks: 5,
        by_source: HashMap::new(),
        sessions_by_agent: BTreeMap::new(),
        model: None,
        skipped: Vec::new(),
        skipped_ignored: 0,
        bench: None,
        bench_provenance: None,
        bench_at: None,
        tiers: Tiers { hook: 1, mcp: 2 },
        hook_threshold: 0.68,
        memory: BTreeMap::new(),
    }
}

pub struct Folder(pub tempfile::TempDir);

impl Folder {
    pub fn new() -> Folder {
        Folder(tempfile::tempdir().unwrap())
    }

    pub fn config(&self) -> std::path::PathBuf {
        self.0.path().join("config.toml")
    }

    pub fn path(&self) -> String {
        self.0.path().display().to_string()
    }

    pub fn view(&self, body: &str) -> ConfigView {
        let config = self.0.path().join("config.toml");
        std::fs::write(&config, format!("sources = [\"{}\"]\n{body}", self.path())).unwrap();
        rhebok::dashboard::core::config::view(&config).unwrap()
    }
}

pub(super) fn no_problems() -> ConfigChecked {
    ConfigChecked {
        errors: Vec::new(),
        blocking: Vec::new(),
    }
}

pub(super) fn fixture(view: ConfigView) -> Arc<FixtureBackend> {
    let fixture = FixtureBackend::default();
    fixture.stats.set(Ok(stats()));
    fixture.config_view.set(Ok(view));
    fixture.config_check.set(Ok(no_problems()));
    Arc::new(fixture)
}

pub(super) fn open(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
    height: f32,
) -> (WindowHandle<Root>, Entity<Shell>, Entity<Settings>) {
    open_wide(cx, fixture, 1400., height)
}

pub(super) fn open_wide(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
    width: f32,
    height: f32,
) -> (WindowHandle<Root>, Entity<Shell>, Entity<Settings>) {
    let (window, shell) = cx.update(|cx| {
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
    });
    cx.run_until_parked();
    click(cx, window, TabId::Settings.button_id());
    let settings = shell
        .read_with(cx, |s, _| s.settings.clone())
        .expect("settings opened");
    render(cx, window);
    (window, shell, settings)
}

pub(super) fn render(cx: &mut TestAppContext, window: WindowHandle<Root>) {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

pub(super) fn wait(cx: &mut TestAppContext, window: WindowHandle<Root>, time: Duration) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
    render(cx, window);
}

pub(super) fn click(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    id: impl Into<SharedString>,
) {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

pub(super) fn type_into(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    id: impl Into<SharedString>,
    text: &str,
) {
    click(cx, window, id);
    cx.update_window(window.into(), |_, window, cx| window.input(text, cx))
        .unwrap();
    cx.run_until_parked();
}

pub(super) fn label(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    id: &str,
) -> Option<String> {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(id)
            .map(|e| e.label().unwrap_or_default().to_string())
    })
    .unwrap()
}

pub(super) fn shown(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> bool {
    label(cx, window, id).is_some()
}

pub(super) fn bounds(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    id: &str,
) -> Bounds<gpui_kit::Pixels> {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.find(id).bounds()
    })
    .unwrap()
}

pub(super) fn checks(fixture: &FixtureBackend) -> Vec<String> {
    fixture
        .requests()
        .into_iter()
        .filter(|r| r.starts_with("config_check"))
        .collect()
}

fn warn_rings(cx: &mut TestAppContext, window: WindowHandle<Root>) -> Vec<Bounds<f32>> {
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let warn = theme::tokens(cx).warn;
        window
            .painted_quads()
            .into_iter()
            .filter(|q| q.border_color == warn && q.border_widths.top.0 > 0.)
            .map(|q| {
                let scale = window.scale_factor();
                Bounds {
                    origin: gpui_kit::point(
                        q.bounds.origin.x.0 / scale,
                        q.bounds.origin.y.0 / scale,
                    ),
                    size: gpui_kit::size(
                        q.bounds.size.width.0 / scale,
                        q.bounds.size.height.0 / scale,
                    ),
                }
            })
            .fold(Vec::new(), |mut rings: Vec<Bounds<f32>>, ring| {
                if !rings.contains(&ring) {
                    rings.push(ring);
                }
                rings
            })
    })
    .unwrap()
}

#[gpui_kit::test]
fn an_edit_shows_the_save_bar_and_its_diff_row(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    assert!(!shown(cx, window, "save-bar"));
    assert_eq!(
        label(cx, window, "settings-title").as_deref(),
        Some("Sources")
    );

    click(cx, window, "toggle-index_transcripts");
    assert_eq!(
        label(cx, window, "save-bar-status").as_deref(),
        Some("1 unsaved change")
    );
    assert!(shown(cx, window, "save-bar-checking"));
    assert_eq!(
        label(cx, window, "field-index_transcripts-edited").as_deref(),
        Some("edited")
    );
    assert_eq!(
        label(cx, window, "settings-nav-sources-edited").as_deref(),
        Some("1")
    );
    assert!(
        checks(&fixture).is_empty(),
        "the check waits for the debounce"
    );

    wait(cx, window, CHECK_DEBOUNCE);
    assert_eq!(checks(&fixture).len(), 1);
    assert!(checks(&fixture)[0].contains("\"index_transcripts\": Bool(true)"));
    assert!(!shown(cx, window, "save-bar-checking"));
    assert!(!shown(cx, window, "diff-preview"));

    click(cx, window, "save-bar-save");
    assert!(shown(cx, window, "diff-preview"));
    assert_eq!(
        label(cx, window, "diff-row-index_transcripts").as_deref(),
        Some("index_transcripts false → true")
    );
    assert!(shown(cx, window, "save-bar-write"));
    assert!(shown(cx, window, "save-bar-back"));

    click(cx, window, "save-bar-discard");
    assert!(!shown(cx, window, "save-bar"));
    assert!(settings.read_with(cx, |s, _| s.pending().is_empty()));
}

#[gpui_kit::test]
fn a_blocking_check_error_disables_save_and_jumps_to_the_field(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let problem = FieldError {
        path: "index_transcripts_max_age_days".into(),
        message: "must be at least 1, got 0".into(),
        line: None,
    };
    fixture.config_check.set(Ok(ConfigChecked {
        errors: vec![problem.clone()],
        blocking: vec![problem],
    }));
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    type_into(cx, window, "input-index_transcripts_max_age_days", "0");
    assert_eq!(
        settings.read_with(cx, |s, _| s
            .pending()
            .get("index_transcripts_max_age_days")
            .map(|v| v.cloned())),
        Some(Some(Value::Integer(0)))
    );
    wait(cx, window, CHECK_DEBOUNCE);
    assert_eq!(
        label(cx, window, "save-bar-fix").as_deref(),
        Some("fix 1 problem to save")
    );
    assert_eq!(
        label(cx, window, "field-index_transcripts_max_age_days-error-0").as_deref(),
        Some("must be at least 1, got 0")
    );
    assert!(shown(cx, window, "settings-nav-sources-problems"));

    click(cx, window, "save-bar-save");
    assert!(!shown(cx, window, "diff-preview"), "Save stays disabled");
    assert!(!settings.read_with(cx, |s, _| s.can_save()));

    settings.update(cx, |s, _| s.scroll_handle().set_offset(Point::default()));
    click(cx, window, "save-bar-fix");
    wait(cx, window, Duration::from_millis(16));
    assert_eq!(
        settings.read_with(cx, |s, _| s.ring().map(str::to_string)),
        Some("index_transcripts_max_age_days".to_string())
    );
    let rings = warn_rings(cx, window);
    assert_eq!(rings.len(), 1, "{rings:?}");
}

#[gpui_kit::test]
fn a_config_changed_save_opens_the_conflict_banner(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let before = folder.view("index_transcripts = false\n");
    let etag = before.0["etag"].as_str().unwrap().to_string();
    let fixture = fixture(before);
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    click(cx, window, "toggle-index_transcripts");
    wait(cx, window, CHECK_DEBOUNCE);

    let after = folder.view("index_transcripts = false\n# edited by hand\n");
    let new_etag = after.0["etag"].as_str().unwrap().to_string();
    assert_ne!(etag, new_etag);
    fixture.config_view.set(Ok(after));
    fixture.config_save.set(Err(DashboardError::ConfigChanged {
        message: "the config file changed since it was read; reload it and make the change again"
            .into(),
        etag: new_etag.clone(),
    }));
    click(cx, window, "save-bar-save");
    click(cx, window, "save-bar-write");
    let saves: Vec<String> = fixture
        .requests()
        .into_iter()
        .filter(|r| r.starts_with("config_save"))
        .collect();
    assert_eq!(saves.len(), 1);
    assert!(
        saves[0].contains(&etag),
        "the save carries the etag it read"
    );
    assert_eq!(
        label(cx, window, "conflict").as_deref(),
        Some("config.toml changed on disk since this page read it, so nothing was saved. Reload to see the new file and lose your 1 unsaved change, or keep editing: your changes stay and the next save applies them on top of the new file.")
    );
    assert!(!shown(cx, window, "diff-preview"));

    click(cx, window, "conflict-keep");
    assert!(!shown(cx, window, "conflict"));
    settings.read_with(cx, |s, _| {
        assert_eq!(s.pending().len(), 1, "keep editing keeps the edits");
        assert_eq!(s.etag(), Some(new_etag.as_str()), "and takes the new etag");
    });

    click(cx, window, "save-bar-save");
    click(cx, window, "save-bar-write");
    assert!(shown(cx, window, "conflict"));
    click(cx, window, "conflict-reload");
    assert!(!shown(cx, window, "conflict"));
    assert!(settings.read_with(cx, |s, _| s.pending().is_empty()));
    assert!(!shown(cx, window, "save-bar"));
}

#[gpui_kit::test]
fn open_setting_scrolls_to_and_rings_the_field(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, shell, settings) = open(cx, fixture, 600.);
    click(cx, window, TabId::Health.button_id());
    shell.update(cx, |s, cx| s.open_setting("index_transcripts", cx));
    render(cx, window);
    wait(cx, window, Duration::from_millis(16));
    assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Settings);
    assert!(settings.read_with(cx, |s, _| s.scroll_handle().offset().y) < px(0.));

    let pane = bounds(cx, window, "settings-pane");
    let field = bounds(cx, window, &field_id("index_transcripts"));
    let centre = |b: Bounds<gpui_kit::Pixels>| f32::from(b.top()) + f32::from(b.size.height) / 2.;
    assert!(
        (centre(field) - centre(pane)).abs() <= 1.,
        "the field is centred in the pane: field {field:?}, pane {pane:?}"
    );
    let rings = warn_rings(cx, window);
    assert_eq!(rings.len(), 1, "one warn ring");
    let ring = rings[0];
    assert_eq!(ring.origin.x, f32::from(field.left()).round() - 2.);
    assert_eq!(ring.size.width, f32::from(field.size.width).round() + 4.);

    wait(cx, window, Duration::from_millis(1600));
    assert!(
        warn_rings(cx, window).is_empty(),
        "the ring fades after 1.6s"
    );
    assert!(settings.read_with(cx, |s, _| s.ring().is_none()));

    shell.update(cx, |s, cx| s.open_setting("sources.0", cx));
    render(cx, window);
    wait(cx, window, Duration::from_millis(16));
    assert_eq!(
        settings.read_with(cx, |s, _| s.ring().map(str::to_string)),
        Some("sources".to_string()),
        "a dotted key walks up to the field that holds it"
    );

    shell.update(cx, |s, cx| s.open_setting("index_transcripts_unknown", cx));
    render(cx, window);
    wait(cx, window, Duration::from_millis(16));
    assert_eq!(
        settings.read_with(cx, |s, _| s.section()),
        Section::Advanced
    );
}

#[gpui_kit::test]
fn the_sources_list_validates_a_new_folder_after_300ms(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture.clone(), 900.);

    type_into(cx, window, "input-add-sources", "/nope/");
    assert!(shown(cx, window, "sources-checking"));
    fixture.config_check.set(Ok(ConfigChecked {
        errors: vec![FieldError {
            path: "sources".into(),
            message: "`/nope` does not exist, and `rhebok index` refuses to run until it does"
                .into(),
            line: None,
        }],
        blocking: Vec::new(),
    }));
    wait(cx, window, Duration::from_millis(299));
    assert!(
        checks(&fixture).is_empty(),
        "nothing is checked before 300ms"
    );
    wait(cx, window, Duration::from_millis(1));
    let sent = checks(&fixture);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].contains("/nope/"),
        "the probe carries the candidate: {}",
        sent[0]
    );
    assert!(!shown(cx, window, "sources-checking"));
    assert_eq!(
        label(cx, window, "sources-candidate-error").as_deref(),
        Some("`/nope` does not exist")
    );
    click(cx, window, "sources-add");
    assert!(
        settings.read_with(cx, |s, _| s.pending().is_empty()),
        "a refused folder is not added"
    );

    type_into(cx, window, "input-add-sources", "x");
    wait(cx, window, Duration::from_millis(200));
    type_into(cx, window, "input-add-sources", "y");
    wait(cx, window, Duration::from_millis(200));
    assert_eq!(
        checks(&fixture).len(),
        1,
        "a keystroke restarts the debounce"
    );
    fixture.config_check.set(Ok(no_problems()));
    wait(cx, window, Duration::from_millis(100));
    assert_eq!(checks(&fixture).len(), 2);
    assert!(!shown(cx, window, "sources-candidate-error"));
    click(cx, window, "sources-add");
    assert_eq!(
        settings.read_with(cx, |s, _| s.pending().get("sources").map(|v| v.cloned())),
        Some(Some(Value::Array(vec![
            Value::String(folder.path()),
            Value::String("/nope/xy".into())
        ])))
    );

    type_into(cx, window, "input-add-sources", "notes");
    wait(cx, window, CHECK_DEBOUNCE);
    assert_eq!(
        label(cx, window, "sources-candidate-error").as_deref(),
        Some("Use an absolute path, or one that starts with ~/.")
    );
}

#[gpui_kit::test]
fn re_index_now_shows_index_status_until_it_finishes(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    fixture.index_start.set(Ok(IndexStarted {
        started: true,
        pid: 42,
        reindex: false,
    }));
    let (window, shell, _) = open(cx, fixture.clone(), 900.);
    assert_eq!(
        label(cx, window, "settings-reindex").as_deref(),
        Some("Re-index now")
    );
    click(cx, window, "settings-reindex");
    assert!(fixture
        .requests()
        .contains(&"index_start reindex=false".to_string()));
    assert_eq!(
        label(cx, window, "settings-reindex").as_deref(),
        Some("Indexing…")
    );
    assert_eq!(
        label(cx, window, "settings-index-status").as_deref(),
        Some("preparing…")
    );
    let runs = shell.read_with(cx, |s, _| s.index_runs());

    fixture.index_status.set(IndexRunView {
        run: Some(IndexRun {
            pid: 42,
            reindex: false,
            finished: true,
            ok: Some(true),
            code: Some(0),
            log: "/tmp/db.log".into(),
        }),
    });
    wait(cx, window, Duration::from_millis(700));
    assert_eq!(
        label(cx, window, "settings-index-status").as_deref(),
        Some("Index finished. The header counts and the graph are refreshed.")
    );
    assert_eq!(
        label(cx, window, "settings-reindex").as_deref(),
        Some("Re-index now")
    );
    assert_eq!(shell.read_with(cx, |s, _| s.index_runs()), runs + 1);
}

#[gpui_kit::test]
fn the_sources_link_opens_the_sources_section_without_a_ring(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, shell, settings) = open(cx, fixture, 900.);
    settings.update(cx, |s, cx| s.pick_section(Section::Advanced, cx));
    click(cx, window, TabId::Integrations.button_id());
    shell.update(cx, |s, cx| s.open_sources(cx));
    render(cx, window);
    wait(cx, window, Duration::from_millis(16));
    assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Settings);
    assert_eq!(settings.read_with(cx, |s, _| s.section()), Section::Sources);
    assert!(settings.read_with(cx, |s, _| s.ring().is_none()));
    assert!(warn_rings(cx, window).is_empty());
}

#[gpui_kit::test]
fn the_index_sentence_wraps_in_a_narrow_window_as_chrome_does(_cx: &mut TestAppContext) {
    let mut cx = TestAppContext::build_with_text_system(
        gpui_kit::TestDispatcher::new(0),
        None,
        Arc::new(gpui_apple::AppleTextSystem::new()),
    );
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, _) = open_wide(&mut cx, fixture, 1024., 900.);
    click(&mut cx, window, "toggle-index_transcripts");
    let note = bounds(&mut cx, window, "settings-index-note");
    let card = bounds(&mut cx, window, "card-index");
    assert_eq!(
        f32::from(note.size.height),
        40.,
        "at 1024 wide Chrome wraps the Index sentence onto two 20px lines: {note:?}"
    );
    assert!(
        note.right() <= card.right() + px(20.),
        "the sentence stays inside its card"
    );
}

#[gpui_kit::test]
fn a_failed_load_stops_waiting_to_focus_a_field(cx: &mut TestAppContext) {
    let fixture = FixtureBackend::default();
    fixture.stats.set(Ok(stats()));
    fixture
        .config_view
        .set(Err(DashboardError::Unavailable("store busy".into())));
    let (window, shell, settings) = open(cx, Arc::new(fixture), 900.);
    assert!(shown(cx, window, "settings-load-error"));
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = notified.clone();
    let _watch = cx.update(|cx| cx.observe(&settings, move |_, _| count.set(count.get() + 1)));
    shell.update(cx, |s, cx| s.open_setting("sources", cx));
    wait(cx, window, Duration::from_millis(48));
    let early = notified.get();
    wait(cx, window, Duration::from_millis(500));
    assert_eq!(
        notified.get(),
        early,
        "nothing re-renders Settings while its config cannot be read"
    );
}

#[gpui_kit::test]
fn a_saved_model_change_offers_a_full_reindex(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let before = folder.view("index_transcripts = false\n");
    let after = folder.view("index_transcripts = false\n[embed]\nmodel = \"nomic-embed-text\"\n");
    let fixture = fixture(before);
    fixture.config_save.set(Ok(after));
    fixture.index_start.set(Ok(IndexStarted {
        started: true,
        pid: 7,
        reindex: true,
    }));
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    settings.update(cx, |s, cx| {
        s.set(
            "embed.model",
            Some(Value::String("nomic-embed-text".into())),
            cx,
        )
    });
    wait(cx, window, CHECK_DEBOUNCE);
    click(cx, window, "save-bar-save");
    assert_eq!(
        label(cx, window, "reindex-warning").as_deref(),
        Some("The embedding model or dimensions change: the index will need a full re-index.")
    );
    click(cx, window, "save-bar-write");
    let notice = label(cx, window, "settings-notice").unwrap_or_default();
    assert!(notice.starts_with("Saved to "), "{notice}");
    assert_eq!(
        label(cx, window, "settings-full-reindex").as_deref(),
        Some("Start full re-index")
    );
    click(cx, window, "settings-full-reindex");
    assert!(fixture
        .requests()
        .contains(&"index_start reindex=true".to_string()));
    assert_eq!(
        label(cx, window, "settings-index-status").as_deref(),
        Some("preparing…")
    );
}

#[test]
fn a_save_through_the_real_core_writes_the_pending_edits() {
    let folder = Folder::new();
    let view = folder.view("index_transcripts = false\n# keep this comment\n");
    let state = super::pending::ConfigState::new(view.0);
    let mut pending = super::pending::Pending::default();
    pending.change(&state, "index_transcripts", Some(Value::Boolean(true)));
    pending.change(
        &state,
        "index_transcripts_max_age_days",
        Some(Value::Integer(30)),
    );
    let config = folder.0.path().join("config.toml");
    let saved =
        rhebok::dashboard::core::config::save(&config, pending.to_save(state.etag())).unwrap();
    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.contains("index_transcripts = true"), "{written}");
    assert!(
        written.contains("index_transcripts_max_age_days = 30"),
        "{written}"
    );
    assert!(written.contains("# keep this comment"), "{written}");
    let saved = super::pending::ConfigState::new(saved.0);
    assert_ne!(saved.etag(), state.etag());
    assert_eq!(saved.file()["index_transcripts"], serde_json::json!(true));
    assert_eq!(
        saved.file()["index_transcripts_max_age_days"],
        serde_json::json!(30)
    );
    let stale = rhebok::dashboard::core::config::save(&config, pending.to_save(state.etag()));
    assert!(matches!(stale, Err(DashboardError::ConfigChanged { .. })));
}
