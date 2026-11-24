use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use rhebok::dashboard::core::stats::Stats;

use crate::app_state::{self, AppState};
use crate::backend::{describe, run_backend};
use crate::chrome::Chrome;
use crate::onboarding::{effective_sources, Onboarding};
use crate::tabs::graph::GraphTab;
use crate::tabs::health::Health;
use crate::tabs::integrations::Integrations;
use crate::tabs::memories::Memories;
use crate::tabs::search::Search;
use crate::tabs::settings::nav::Section;
use crate::tabs::settings::Settings;
use crate::theme::{tokens, Type, MONO, SANS};
use crate::version_gate::Gate;
use crate::widgets::css_box::css_rule_bottom;
use crate::widgets::{banner, baseline, exact_text, tracked};

pub const LOADING: &str = "Loading…";

static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../dashboard/src/assets/rhebok-icon.png").to_vec(),
    ))
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabId {
    #[default]
    Graph,
    Search,
    Memories,
    Integrations,
    Health,
    Settings,
}

impl TabId {
    pub const ALL: [TabId; 6] = [
        TabId::Graph,
        TabId::Search,
        TabId::Memories,
        TabId::Integrations,
        TabId::Health,
        TabId::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TabId::Graph => "Graph",
            TabId::Search => "Search",
            TabId::Memories => "Memories",
            TabId::Integrations => "Integrations",
            TabId::Health => "Health",
            TabId::Settings => "Settings",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            TabId::Graph => "graph",
            TabId::Search => "search",
            TabId::Memories => "memories",
            TabId::Integrations => "integrations",
            TabId::Health => "health",
            TabId::Settings => "settings",
        }
    }

    pub fn button_id(self) -> SharedString {
        format!("tab-{}", self.id()).into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupState {
    Deciding,
    Open,
    Closed,
}

enum Setup {
    Deciding,
    Open(Entity<Onboarding>),
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRequest {
    pub key: String,
    pub n: u64,
}

pub struct Shell {
    gate: Gate,
    tab: TabId,
    pub graph: Option<Entity<GraphTab>>,
    pub search: Option<Entity<Search>>,
    pub memories: Option<Entity<Memories>>,
    pub integrations: Option<Entity<Integrations>>,
    pub health: Option<Entity<Health>>,
    pub settings: Option<Entity<Settings>>,
    stats: Option<Stats>,
    stats_error: Option<String>,
    stats_task: Option<Task<()>>,
    setting_request: Option<SettingRequest>,
    index_runs: u64,
    memory_writes: u64,
    setup_requests: u64,
    setup: Setup,
    dismissed: bool,
    setup_task: Option<Task<()>>,
    highlight: HashSet<String>,
    chrome: Chrome,
    window_drag_armed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabShown(pub TabId);

impl EventEmitter<TabShown> for Shell {}

pub fn counts(stats: Option<&Stats>, error: Option<&str>) -> String {
    match (stats, error) {
        (Some(s), _) => format!("{} docs · {} chunks", s.documents, grouped(s.chunks)),
        (None, Some(_)) => "stats unavailable".to_string(),
        (None, None) => "—".to_string(),
    }
}

pub fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (ix, ch) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

impl Shell {
    pub fn new(gate: Gate, cx: &mut Context<Self>) -> Shell {
        let mut shell = Shell {
            gate,
            tab: TabId::default(),
            graph: None,
            search: None,
            memories: None,
            integrations: None,
            health: None,
            settings: None,
            stats: None,
            stats_error: None,
            stats_task: None,
            setting_request: None,
            index_runs: 0,
            memory_writes: 0,
            setup_requests: 0,
            setup: Setup::Deciding,
            dismissed: false,
            setup_task: None,
            highlight: HashSet::new(),
            chrome: Chrome::Plain,
            window_drag_armed: false,
        };
        if shell.gate == Gate::Ok {
            shell.dismissed = app_state::installed_path(cx)
                .is_some_and(|path| app_state::load(&path).setup_dismissed);
            shell.decide(cx);
            shell.refresh_stats(cx);
        }
        shell
    }

    pub fn setup(&self) -> SetupState {
        match self.setup {
            Setup::Deciding => SetupState::Deciding,
            Setup::Open(_) => SetupState::Open,
            Setup::Closed => SetupState::Closed,
        }
    }

    pub fn onboarding(&self) -> Option<Entity<Onboarding>> {
        match &self.setup {
            Setup::Open(onboarding) => Some(onboarding.clone()),
            _ => None,
        }
    }

    fn decide(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.setup, Setup::Deciding) {
            return;
        }
        if self.dismissed || self.stats_error.is_some() {
            self.close_setup(cx);
            return;
        }
        let Some(stats) = &self.stats else {
            return;
        };
        if stats.documents > 0 {
            self.close_setup(cx);
            return;
        }
        if self.setup_task.is_some() {
            return;
        }
        self.setup_task = Some(run_backend(
            cx,
            |backend| backend.config_view(),
            |shell: &mut Shell, result, cx| {
                let empty = result.is_ok_and(|view| effective_sources(&view.0).is_empty());
                if !matches!(shell.setup, Setup::Deciding) {
                    return;
                }
                if empty {
                    shell.open_setup(cx);
                } else {
                    shell.close_setup(cx);
                }
            },
        ));
    }

    fn open_setup(&mut self, cx: &mut Context<Self>) {
        if matches!(self.setup, Setup::Open(_)) {
            return;
        }
        let shell = cx.entity().downgrade();
        self.setup = Setup::Open(cx.new(|cx| Onboarding::new(shell, cx)));
        cx.notify();
    }

    fn close_setup(&mut self, cx: &mut Context<Self>) {
        self.setup = Setup::Closed;
        self.ensure(TabId::Graph, cx);
        self.ensure(self.tab, cx);
        cx.emit(TabShown(self.tab));
        cx.notify();
    }

    fn dismiss_setup(&mut self, cx: &mut Context<Self>) {
        self.dismissed = true;
        let Some(path) = app_state::installed_path(cx) else {
            return;
        };
        if let Err(e) = app_state::save(
            &path,
            AppState {
                setup_dismissed: true,
            },
        ) {
            eprintln!("rhebok-app: {}: {e}", path.display());
        }
    }

    pub fn skip_setup(&mut self, cx: &mut Context<Self>) {
        self.dismiss_setup(cx);
        self.close_setup(cx);
    }

    pub fn finish_setup(&mut self, query: String, cx: &mut Context<Self>) {
        self.dismiss_setup(cx);
        self.refresh_stats(cx);
        self.index_runs += 1;
        self.tab = TabId::Search;
        self.close_setup(cx);
        if let Some(search) = self.search.clone() {
            search.update(cx, |search, cx| search.seed(query, cx));
        }
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }

    pub fn stats(&self) -> Option<&Stats> {
        self.stats.as_ref()
    }

    pub fn stats_error(&self) -> Option<&str> {
        self.stats_error.as_deref()
    }

    pub fn setting_request(&self) -> Option<&SettingRequest> {
        self.setting_request.as_ref()
    }

    pub fn index_runs(&self) -> u64 {
        self.index_runs
    }

    pub fn memory_writes(&self) -> u64 {
        self.memory_writes
    }

    pub fn memory_written(&mut self, cx: &mut Context<Self>) {
        self.memory_writes += 1;
        self.refresh_stats(cx);
        cx.notify();
    }

    pub fn setup_requests(&self) -> u64 {
        self.setup_requests
    }

    pub fn request_setup(&mut self, cx: &mut Context<Self>) {
        if self.gate != Gate::Ok {
            return;
        }
        self.setup_requests += 1;
        self.open_setup(cx);
    }

    pub fn highlight(&self) -> &HashSet<String> {
        &self.highlight
    }

    pub fn set_highlight(&mut self, doc_ids: Vec<String>, cx: &mut Context<Self>) {
        self.highlight = doc_ids.into_iter().collect();
        cx.notify();
    }

    pub fn index_finished(&mut self, cx: &mut Context<Self>) {
        self.index_runs += 1;
        self.refresh_stats(cx);
    }

    pub fn counts_text(&self) -> String {
        counts(self.stats.as_ref(), self.stats_error.as_deref())
    }

    pub fn refresh_stats(&mut self, cx: &mut Context<Self>) {
        self.stats_task = Some(run_backend(
            cx,
            |backend| backend.stats(),
            |shell: &mut Shell, result, cx| {
                match result {
                    Ok(stats) => {
                        shell.stats = Some(stats);
                        shell.stats_error = None;
                    }
                    Err(e) => shell.stats_error = Some(describe(&e)),
                }
                shell.decide(cx);
            },
        ));
    }

    pub fn open_tab(&mut self, tab: TabId, cx: &mut Context<Self>) {
        if self.gate != Gate::Ok {
            return;
        }
        self.tab = tab;
        match self.setup {
            Setup::Deciding => {}
            Setup::Open(_) => self.close_setup(cx),
            Setup::Closed => {
                self.ensure(tab, cx);
                cx.emit(TabShown(tab));
            }
        }
        cx.notify();
    }

    pub fn open_setting(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.gate != Gate::Ok {
            return;
        }
        let n = self.setting_request.as_ref().map_or(1, |r| r.n + 1);
        self.setting_request = Some(SettingRequest {
            key: key.to_string(),
            n,
        });
        self.open_tab(TabId::Settings, cx);
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |settings, cx| settings.focus_field(key, cx));
        }
    }

    pub fn open_sources(&mut self, cx: &mut Context<Self>) {
        if self.gate != Gate::Ok {
            return;
        }
        self.open_tab(TabId::Settings, cx);
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |settings, cx| {
                settings.pick_section(Section::Sources, cx)
            });
        }
    }

    fn ensure(&mut self, tab: TabId, cx: &mut Context<Self>) {
        if self.gate != Gate::Ok {
            return;
        }
        if tab == TabId::Graph {
            if self.graph.is_none() {
                let shell = cx.entity();
                self.graph = Some(cx.new(|cx| GraphTab::new(shell, cx)));
            }
            return;
        }
        if tab == TabId::Search {
            if self.search.is_none() {
                let shell = cx.entity().downgrade();
                self.search = Some(cx.new(|_| Search::new(shell)));
            }
            return;
        }
        if tab == TabId::Memories {
            if self.memories.is_none() {
                let shell = cx.entity().downgrade();
                self.memories = Some(cx.new(|cx| Memories::new(shell, cx)));
            }
            return;
        }
        if tab == TabId::Integrations {
            if self.integrations.is_none() {
                let shell = cx.entity();
                self.integrations = Some(cx.new(|cx| Integrations::new(shell, cx)));
            }
            return;
        }
        if tab == TabId::Health {
            if self.health.is_none() {
                let shell = cx.entity();
                self.health = Some(cx.new(|cx| Health::new(shell, cx)));
            }
            return;
        }
        if tab == TabId::Settings && self.settings.is_none() {
            let shell = cx.entity().downgrade();
            self.settings = Some(cx.new(|cx| Settings::new(shell, cx)));
        }
    }

    fn active_view(&self) -> Option<AnyView> {
        match self.tab {
            TabId::Graph => self.graph.clone().map(AnyView::from),
            TabId::Search => self.search.clone().map(AnyView::from),
            TabId::Memories => self.memories.clone().map(AnyView::from),
            TabId::Integrations => self.integrations.clone().map(AnyView::from),
            TabId::Health => self.health.clone().map(AnyView::from),
            TabId::Settings => self.settings.clone().map(AnyView::from),
        }
    }

    pub fn with_chrome(mut self, chrome: Chrome) -> Shell {
        self.chrome = chrome;
        self
    }

    pub fn chrome(&self) -> Chrome {
        self.chrome
    }

    fn drags_the_window<E: InteractiveElement>(&self, area: E, cx: &mut Context<Self>) -> E {
        area.on_mouse_down(
            MouseButton::Left,
            cx.listener(|shell, event: &MouseDownEvent, window, _| {
                shell.window_drag_armed = event.click_count < 2;
                if event.click_count == 2 {
                    window.titlebar_double_click();
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|shell, _: &MouseUpEvent, _, _| shell.window_drag_armed = false),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|shell, _: &MouseUpEvent, _, _| shell.window_drag_armed = false),
        )
        .on_mouse_move(cx.listener(|shell, event: &MouseMoveEvent, window, _| {
            if !event.dragging() {
                shell.window_drag_armed = false;
            } else if std::mem::take(&mut shell.window_drag_armed) {
                window.start_window_move();
            }
        }))
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let counts = self.counts_text();
        div()
            .id("header")
            .test_support()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(32.))
            .h(px(56.))
            .pl(px(self.chrome.header_inset()))
            .pr(px(24.))
            .relative()
            .when(self.chrome == Chrome::TrafficLights, |d| {
                self.drags_the_window(d, cx)
            })
            .bg(t.ground)
            .border_b_1()
            .border_color(transparent_black())
            .child(css_rule_bottom(t.line))
            .child(
                div()
                    .id("logo")
                    .test_support()
                    .aria_label("Rhebok")
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(img(LOGO.clone()).size(px(32.)))
                    .child(
                        tracked("Rhebok", SANS, FontWeight::MEDIUM, px(22.), -0.02)
                            .line_height(px(33.))
                            .text_color(t.ink),
                    ),
            )
            .child(
                div()
                    .id("nav")
                    .flex()
                    .gap(px(4.))
                    .h_full()
                    .items_start()
                    .children(TabId::ALL.map(|tab| {
                        let active = tab == self.tab && !matches!(self.setup, Setup::Open(_));
                        div()
                            .id(tab.button_id())
                            .test_support()
                            .role(Role::Tab)
                            .aria_label(tab.label())
                            .aria_selected(active)
                            .flex()
                            .items_center()
                            .h(px(56.))
                            .px(px(12.))
                            .type_px(15.)
                            .border_b_2()
                            .cursor_pointer()
                            .occlude()
                            .map(|d| {
                                if active {
                                    d.semibold().text_color(t.ink).border_color(t.sun)
                                } else {
                                    d.medium()
                                        .text_color(t.muted)
                                        .border_color(transparent_black())
                                        .hover(|s| s.text_color(t.ink))
                                }
                            })
                            .on_click(cx.listener(move |shell, _, _, cx| shell.open_tab(tab, cx)))
                            .child(exact_text(tab.label()))
                    })),
            )
            .child(div().flex_grow(1.))
            .child(
                div()
                    .id("counts")
                    .test_support()
                    .aria_label(counts.clone())
                    .child(
                        tracked(counts, MONO, FontWeight::NORMAL, px(12.), 0.06)
                            .line_height(px(16.))
                            .text_color(t.muted),
                    ),
            )
    }

    fn gate_notice(&self) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p(px(16.))
            .child(banner::error("version-gate", gate_message(&self.gate)).max_w(px(560.)))
    }
}

fn version_parts(version: &str) -> Option<Vec<u64>> {
    version
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

pub fn gate_message(gate: &Gate) -> String {
    match gate {
        Gate::Ok => String::new(),
        Gate::Missing { fix } | Gate::Unanswered { fix } => fix.clone(),
        Gate::Mismatch { cli, app } => match (version_parts(cli), version_parts(app)) {
            (Some(c), Some(a)) if c < a => {
                format!("rhebok {cli} is older than rhebok-app {app}; run rhebok update")
            }
            (Some(c), Some(a)) if a < c => {
                format!("rhebok-app {app} is older than rhebok {cli}; run rhebok update")
            }
            _ => format!("rhebok {cli} does not match rhebok-app {app}; run rhebok update"),
        },
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let root = div()
            .id("shell")
            .size_full()
            .flex()
            .flex_col()
            .bg(t.ground)
            .text_color(t.ink)
            .line_height(relative(1.5));
        if self.gate != Gate::Ok {
            return root
                .when(self.chrome == Chrome::TrafficLights, |d| {
                    self.drags_the_window(d, cx)
                })
                .child(self.gate_notice());
        }
        let view = match &self.setup {
            Setup::Deciding => None,
            Setup::Open(onboarding) => Some(AnyView::from(onboarding.clone())),
            Setup::Closed => self.active_view(),
        };
        let deciding = matches!(self.setup, Setup::Deciding);
        root.child(self.header(cx)).child(
            div()
                .id("main")
                .flex_grow(1.)
                .min_h(px(0.))
                .p(px(16.))
                .when(deciding, |d| {
                    d.child(
                        div()
                            .id("setup-deciding")
                            .test_support()
                            .aria_label(LOADING)
                            .p(px(16.))
                            .size_sm()
                            .text_color(t.muted)
                            .child(baseline::text(LOADING)),
                    )
                })
                .when_some(view, |d, view| d.child(view)),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;

    use gpui_kit::base::Root;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        point, px, size, AppContext, Bounds, Entity, MouseButton, MouseDownEvent, MouseMoveEvent,
        MouseUpEvent, Pixels, PlatformInput, Point, SharedString, TestAppContext, WindowBounds,
        WindowHandle, WindowOptions,
    };
    use rhebok::dashboard::core::stats::{Stats, Tiers};
    use rhebok::dashboard::core::DashboardError;

    use super::{gate_message, grouped, Shell, TabId};
    use crate::backend::{self, FixtureBackend};
    use crate::chrome::{self, Chrome};
    use crate::theme;
    use crate::version_gate::Gate;

    fn stats(documents: i64, chunks: i64) -> Stats {
        Stats {
            documents,
            chunks,
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

    fn fixture(documents: i64, chunks: i64) -> Arc<FixtureBackend> {
        let fixture = FixtureBackend::default();
        fixture.stats.set(Ok(stats(documents, chunks)));
        Arc::new(fixture)
    }

    fn open(
        cx: &mut TestAppContext,
        fixture: Arc<FixtureBackend>,
        gate: Gate,
    ) -> (WindowHandle<Root>, Entity<Shell>) {
        cx.update(|cx| backend::install(cx, fixture));
        open_without_backend(cx, gate)
    }

    fn open_without_backend(
        cx: &mut TestAppContext,
        gate: Gate,
    ) -> (WindowHandle<Root>, Entity<Shell>) {
        cx.update(|cx| {
            if !cx.has_global::<theme::Tokens>() {
                theme::install(cx);
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
                |_, cx| cx.new(|cx| Shell::new(gate, cx)),
            )
            .expect("open test window");
            (window.downcast::<Root>().expect("Base Root"), shell)
        })
    }

    fn counts_label(cx: &mut TestAppContext, window: WindowHandle<Root>) -> String {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window
                .find("counts")
                .label()
                .unwrap_or_default()
                .to_string()
        })
        .unwrap()
    }

    fn click(cx: &mut TestAppContext, window: WindowHandle<Root>, id: SharedString) {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn graph_is_the_default_tab(cx: &mut TestAppContext) {
        let (window, shell) = open(cx, fixture(4, 1234), Gate::Ok);
        cx.run_until_parked();
        assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Graph);
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("graph").visible());
            assert!(window.try_find("search").is_none());
            assert_eq!(window.find("tab-graph").selected(), Some(true));
            assert_eq!(window.find("tab-search").selected(), Some(false));
        })
        .unwrap();
    }

    fn open_with_chrome(
        cx: &mut TestAppContext,
        chrome: Chrome,
    ) -> (WindowHandle<Root>, Entity<Shell>) {
        cx.update(|cx| {
            backend::install(cx, fixture(4, 1234));
            theme::install(cx);
            let (window, shell) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1400.), px(900.)),
                    })),
                    titlebar: Some(chrome::titlebar()),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|cx| Shell::new(Gate::Ok, cx).with_chrome(chrome)),
            )
            .expect("open test window");
            (window.downcast::<Root>().expect("Base Root"), shell)
        })
    }

    fn header_layout(cx: &mut TestAppContext, window: WindowHandle<Root>) -> Vec<Bounds<Pixels>> {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            ["header", "logo", "tab-graph", "counts"]
                .map(|id| window.find(id).bounds())
                .to_vec()
        })
        .unwrap()
    }

    #[gpui_kit::test]
    fn only_a_window_with_traffic_lights_moves_the_logo_right_to_clear_them(
        cx: &mut TestAppContext,
    ) {
        let (plain, shell) = open_with_chrome(cx, Chrome::Plain);
        cx.run_until_parked();
        assert_eq!(shell.read_with(cx, |s, _| s.chrome()), Chrome::Plain);
        let plain = header_layout(cx, plain);
        let (lit, _) = open_with_chrome(cx, Chrome::TrafficLights);
        cx.run_until_parked();
        let lit = header_layout(cx, lit);

        assert_eq!(plain[1].origin.x, px(chrome::HEADER_INSET));
        assert_eq!(
            lit[1].origin.x,
            px(chrome::HEADER_INSET_BESIDE_TRAFFIC_LIGHTS)
        );
        let shift = lit[1].origin.x - plain[1].origin.x;
        assert_eq!(lit[0], plain[0]);
        assert_eq!(lit[0].size.height, px(chrome::HEADER_HEIGHT));
        for (lit, plain) in lit.iter().zip(&plain).skip(1).take(2) {
            assert_eq!(lit.size, plain.size);
            assert_eq!(lit.origin.y, plain.origin.y);
            assert_eq!(lit.origin.x - plain.origin.x, shift);
        }
        assert_eq!(lit[3], plain[3]);
    }

    fn dispatch(cx: &mut TestAppContext, window: WindowHandle<Root>, event: PlatformInput) {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.dispatch_event(event, cx);
        })
        .unwrap();
    }

    fn press(position: Point<Pixels>, click_count: usize) -> PlatformInput {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            click_count,
            ..Default::default()
        })
    }

    fn armed(cx: &mut TestAppContext, shell: &Entity<Shell>) -> bool {
        shell.read_with(cx, |s, _| s.window_drag_armed)
    }

    #[gpui_kit::test]
    fn a_header_press_released_anywhere_else_never_drags_the_window_later(cx: &mut TestAppContext) {
        let (window, shell) = open_with_chrome(cx, Chrome::TrafficLights);
        cx.run_until_parked();
        let counts = header_layout(cx, window)[3];
        let empty = point(counts.origin.x - px(40.), px(chrome::HEADER_HEIGHT / 2.));
        let below = point(empty.x, px(400.));

        dispatch(cx, window, press(empty, 1));
        assert!(armed(cx, &shell));
        dispatch(
            cx,
            window,
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: below,
                click_count: 1,
                ..Default::default()
            }),
        );
        assert!(!armed(cx, &shell));

        dispatch(cx, window, press(empty, 1));
        assert!(armed(cx, &shell));
        dispatch(
            cx,
            window,
            PlatformInput::MouseMove(MouseMoveEvent {
                position: empty,
                pressed_button: None,
                ..Default::default()
            }),
        );
        assert!(!armed(cx, &shell));

        dispatch(cx, window, press(empty, 2));
        assert!(!armed(cx, &shell));
    }

    #[gpui_kit::test]
    fn tabs_still_switch_in_a_window_whose_header_drags_it(cx: &mut TestAppContext) {
        let (window, shell) = open_with_chrome(cx, Chrome::TrafficLights);
        cx.run_until_parked();
        click(cx, window, TabId::Search.button_id());
        assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Search);
        click(cx, window, TabId::Graph.button_id());
        assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Graph);
    }

    #[gpui_kit::test]
    fn the_header_shows_counts_or_a_dash(cx: &mut TestAppContext) {
        let (window, _) = open(cx, fixture(4, 1234), Gate::Ok);
        assert_eq!(counts_label(cx, window), "—");
        cx.run_until_parked();
        assert_eq!(counts_label(cx, window), "4 docs · 1,234 chunks");

        let failing = FixtureBackend::default();
        failing
            .stats
            .set(Err(DashboardError::Unavailable("store busy".into())));
        let (window, _) = open(cx, Arc::new(failing), Gate::Ok);
        cx.run_until_parked();
        assert_eq!(counts_label(cx, window), "stats unavailable");
    }

    #[test]
    fn chunk_counts_group_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1234), "1,234");
        assert_eq!(grouped(34_174), "34,174");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[gpui_kit::test]
    fn switching_tabs_keeps_each_tabs_state(cx: &mut TestAppContext) {
        let (window, shell) = open(cx, fixture(4, 1234), Gate::Ok);
        cx.run_until_parked();
        click(cx, window, TabId::Search.button_id());
        let search = shell
            .read_with(cx, |s, _| s.search.clone())
            .expect("search opened");
        search.update(cx, |s, cx| s.seed("how does the hook gate work".into(), cx));
        cx.run_until_parked();
        click(cx, window, TabId::Health.button_id());
        assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Health);
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("search").is_none());
            assert!(window.find("health").visible());
        })
        .unwrap();
        click(cx, window, TabId::Search.button_id());
        let again = shell
            .read_with(cx, |s, _| s.search.clone())
            .expect("search kept");
        assert_eq!(again.entity_id(), search.entity_id());
        assert_eq!(
            again.read_with(cx, |s, _| s.query().to_string()),
            "how does the hook gate work"
        );
    }

    #[gpui_kit::test]
    fn a_slow_backend_call_never_blocks_input(cx: &mut TestAppContext) {
        let slow = fixture(4, 1234);
        let release = slow.stats.hold();
        let (window, shell) = open(cx, slow.clone(), Gate::Ok);
        click(cx, window, TabId::Search.button_id());
        assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Search);
        assert_eq!(counts_label(cx, window), "—");
        slow.note("click processed");
        release.send(()).unwrap();
        let executor = cx.executor();
        let dispatcher = executor.dispatcher().as_test().expect("test dispatcher");
        while dispatcher.tick(true) {}
        assert_eq!(
            slow.stats.calls(),
            1,
            "the stats call did not run on the background executor"
        );
        assert_eq!(counts_label(cx, window), "—");
        cx.run_until_parked();
        let journal: Vec<String> = slow
            .journal()
            .into_iter()
            .filter(|e| e == "click processed" || e == "stats returned")
            .collect();
        assert_eq!(journal, ["click processed", "stats returned"]);
        assert_eq!(counts_label(cx, window), "4 docs · 1,234 chunks");
    }

    #[gpui_kit::test]
    fn a_gated_window_without_a_backend_survives_tab_clicks(cx: &mut TestAppContext) {
        let (window, shell) = open_without_backend(
            cx,
            Gate::Mismatch {
                cli: "0.1.9".into(),
                app: "0.2.0".into(),
            },
        );
        for tab in TabId::ALL {
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                if window.try_find(tab.button_id()).is_some() {
                    window.click(tab.button_id(), cx);
                }
            })
            .unwrap();
            shell.update(cx, |s, cx| s.open_tab(tab, cx));
        }
        shell.update(cx, |s, cx| s.open_setting("embed.model", cx));
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("version-gate").visible());
            assert!(window.try_find("header").is_none());
            for tab in TabId::ALL {
                assert!(window.try_find(tab.id()).is_none());
            }
        })
        .unwrap();
        shell.read_with(cx, |s, _| {
            assert!(s.graph.is_none() && s.search.is_none() && s.settings.is_none());
            assert!(s.setting_request().is_none());
        });

        let (window, shell) = open_without_backend(cx, Gate::Ok);
        cx.run_until_parked();
        click(cx, window, TabId::Health.button_id());
        assert_eq!(shell.read_with(cx, |s, _| s.tab()), TabId::Health);
        assert_eq!(counts_label(cx, window), "—");
    }

    #[test]
    fn the_mismatch_advice_names_the_older_side() {
        let mismatch = |cli: &str, app: &str| {
            gate_message(&Gate::Mismatch {
                cli: cli.into(),
                app: app.into(),
            })
        };
        assert_eq!(
            mismatch("0.1.9", "0.2.0"),
            "rhebok 0.1.9 is older than rhebok-app 0.2.0; run rhebok update"
        );
        assert_eq!(
            mismatch("0.10.0", "0.9.0"),
            "rhebok-app 0.9.0 is older than rhebok 0.10.0; run rhebok update"
        );
        assert_eq!(
            mismatch("0.2.0-rc1", "0.2.0"),
            "rhebok 0.2.0-rc1 does not match rhebok-app 0.2.0; run rhebok update"
        );
        assert!(!mismatch("0.1.9", "0.2.0").contains('`'));
    }

    #[gpui_kit::test]
    fn open_setting_switches_to_settings(cx: &mut TestAppContext) {
        let (_, shell) = open(cx, fixture(4, 1234), Gate::Ok);
        shell.update(cx, |s, cx| s.open_setting("embed.model", cx));
        shell.read_with(cx, |s, _| {
            assert_eq!(s.tab(), TabId::Settings);
            assert_eq!(
                s.setting_request().map(|r| r.key.as_str()),
                Some("embed.model")
            );
        });
    }

    #[gpui_kit::test]
    fn a_mismatched_cli_shows_only_the_update_notice(cx: &mut TestAppContext) {
        let backend = fixture(4, 1234);
        let (window, _) = open(
            cx,
            backend.clone(),
            Gate::Mismatch {
                cli: "0.1.9".into(),
                app: "0.2.0".into(),
            },
        );
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let notice = window.find("version-gate");
            assert!(notice.visible());
            assert_eq!(
                notice.label(),
                Some("rhebok 0.1.9 is older than rhebok-app 0.2.0; run rhebok update")
            );
            assert!(window.try_find("header").is_none());
            assert!(window.try_find("graph").is_none());
        })
        .unwrap();
        assert_eq!(backend.stats.calls(), 0);
        assert!(backend.requests().is_empty());
    }

    #[gpui_kit::test]
    fn an_unanswered_cli_says_so_instead_of_asking_for_an_install(cx: &mut TestAppContext) {
        let backend = fixture(4, 1234);
        let fix = "rhebok at /x did not answer within 3 seconds; open Rhebok again, and if it keeps happening, run \"/x\" --version in a terminal";
        let (window, _) = open(cx, backend.clone(), Gate::Unanswered { fix: fix.into() });
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("version-gate").label(), Some(fix));
            assert!(window.try_find("header").is_none());
        })
        .unwrap();
        assert_eq!(backend.stats.calls(), 0);
    }

    #[gpui_kit::test]
    fn a_missing_cli_shows_its_fix_and_calls_nothing(cx: &mut TestAppContext) {
        let backend = fixture(4, 1234);
        let (window, _) = open(
            cx,
            backend.clone(),
            Gate::Missing {
                fix: "rhebok is not installed at /x; run the rhebok installer, then open Rhebok again"
                    .into(),
            },
        );
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window
                .find("version-gate")
                .label()
                .unwrap()
                .contains("run the rhebok installer"));
            assert!(window.try_find("header").is_none());
        })
        .unwrap();
        assert_eq!(backend.stats.calls(), 0);
    }
}
