use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HOLD_LIMIT: Duration = Duration::from_secs(5);
pub const READ_RETRY_PAUSE: Duration = Duration::from_millis(500);

use gpui_kit::{App, Context, Global, Task};
use rhebok::config::Config;
use rhebok::dashboard::core::agents::{AgentChanged, AgentList, AgentRequest};
use rhebok::dashboard::core::config::{
    ConfigCheck, ConfigChecked, ConfigSave, ConfigSaved, ConfigView, EmbedEndpoint,
    EmbedEndpointSaved,
};
use rhebok::dashboard::core::document::Document;
use rhebok::dashboard::core::embed::EmbedTest;
use rhebok::dashboard::core::graph::Graph;
use rhebok::dashboard::core::ignore::IgnorePreview;
use rhebok::dashboard::core::index::{IndexRunView, IndexStarted};
use rhebok::dashboard::core::install::{EmbedView, InstallConfig, InstallView};
use rhebok::dashboard::core::memories::{Memories, MemoryReach};
use rhebok::dashboard::core::memory_write::{
    DeleteOutcome, DeleteRequest, SaveOutcome, SaveRequest,
};
use rhebok::dashboard::core::search::{SearchOutcome, SearchQuery};
use rhebok::dashboard::core::stats::{Progress, Stats};
use rhebok::dashboard::core::update::UpdateStarted;
use rhebok::dashboard::core::version::Version;
use rhebok::dashboard::core::{self as core, DashboardError};
use rhebok::setup::agents::AgentEnv;
use rhebok::setup::Paths;

pub type Answer<T> = Result<T, DashboardError>;

pub trait Backend: Send + Sync + 'static {
    fn stats(&self) -> Answer<Stats>;
    fn progress(&self) -> Answer<Progress>;
    fn graph(&self) -> Answer<Graph>;
    fn document(&self, id: String) -> Answer<Document>;
    fn version(&self, refresh: bool) -> Answer<Version>;
    fn search(&self, query: SearchQuery) -> Answer<SearchOutcome>;
    fn memories(&self) -> Memories;
    fn memory_reach(&self, id: String) -> Answer<MemoryReach>;
    fn memory_save(&self, request: SaveRequest) -> Answer<SaveOutcome>;
    fn memory_delete(&self, request: DeleteRequest) -> Answer<DeleteOutcome>;
    fn config_view(&self) -> Answer<ConfigView>;
    fn config_save(&self, request: ConfigSave) -> Answer<ConfigSaved>;
    fn config_check(&self, request: ConfigCheck) -> Answer<ConfigChecked>;
    fn set_embed_endpoint(&self, request: EmbedEndpoint) -> Answer<EmbedEndpointSaved>;
    fn embed_test(&self) -> EmbedTest;
    fn ignore_preview(&self, pattern: String) -> Answer<IgnorePreview>;
    fn agents(&self) -> AgentList;
    fn agent_connect(&self, request: AgentRequest) -> Answer<AgentChanged>;
    fn agent_disconnect(&self, request: AgentRequest) -> Answer<AgentChanged>;
    fn index_start(&self, reindex: bool) -> Answer<IndexStarted>;
    fn index_status(&self) -> IndexRunView;
    fn update_start(&self) -> Answer<UpdateStarted>;
    fn restart(&self) -> Answer<AppRestarted>;
    fn install(&self) -> InstallView;
}

#[derive(Debug, Clone)]
pub struct AppRestarted {
    pub pid: u32,
    pub report: String,
}

pub struct CoreBackend {
    pub config_path: PathBuf,
    pub cli: PathBuf,
    pub agents: Option<AgentEnv>,
}

impl CoreBackend {
    pub fn from_env() -> CoreBackend {
        CoreBackend {
            config_path: Config::config_path(),
            cli: Paths::from_env().bin,
            agents: None,
        }
    }

    fn config(&self) -> Config {
        Config::load_from(&self.config_path)
    }

    fn agent_env(&self) -> AgentEnv {
        self.agents.clone().unwrap_or_else(AgentEnv::from_env)
    }
}

impl Backend for CoreBackend {
    fn stats(&self) -> Answer<Stats> {
        core::stats::stats(&self.config())
    }

    fn progress(&self) -> Answer<Progress> {
        core::stats::progress()
    }

    fn graph(&self) -> Answer<Graph> {
        core::graph::graph(&self.config())
    }

    fn document(&self, id: String) -> Answer<Document> {
        core::document::document(&self.config(), &id)
    }

    fn version(&self, refresh: bool) -> Answer<Version> {
        core::version::version(refresh)
    }

    fn search(&self, query: SearchQuery) -> Answer<SearchOutcome> {
        core::search::search(&self.config(), &query)
    }

    fn memories(&self) -> Memories {
        core::memories::list(&self.config())
    }

    fn memory_reach(&self, id: String) -> Answer<MemoryReach> {
        core::memories::reach(&self.config(), &id)
    }

    fn memory_save(&self, request: SaveRequest) -> Answer<SaveOutcome> {
        core::memory_write::save(&self.config(), request)
    }

    fn memory_delete(&self, request: DeleteRequest) -> Answer<DeleteOutcome> {
        core::memory_write::delete(&self.config(), request)
    }

    fn config_view(&self) -> Answer<ConfigView> {
        core::config::view(&self.config_path)
    }

    fn config_save(&self, request: ConfigSave) -> Answer<ConfigSaved> {
        core::config::save(&self.config_path, request)
    }

    fn config_check(&self, request: ConfigCheck) -> Answer<ConfigChecked> {
        core::config::check(&self.config_path, request)
    }

    fn set_embed_endpoint(&self, request: EmbedEndpoint) -> Answer<EmbedEndpointSaved> {
        core::config::set_embed_endpoint(&self.config_path, request)
    }

    fn embed_test(&self) -> EmbedTest {
        core::embed::test(&self.config_path)
    }

    fn ignore_preview(&self, pattern: String) -> Answer<IgnorePreview> {
        core::ignore::preview(&self.config(), &pattern)
    }

    fn agents(&self) -> AgentList {
        core::agents::list(&self.agent_env())
    }

    fn agent_connect(&self, request: AgentRequest) -> Answer<AgentChanged> {
        core::agents::connect(&self.agent_env(), request)
    }

    fn agent_disconnect(&self, request: AgentRequest) -> Answer<AgentChanged> {
        core::agents::disconnect(&self.agent_env(), request)
    }

    fn index_start(&self, reindex: bool) -> Answer<IndexStarted> {
        core::index::start(&Config::db_path(), &self.cli, reindex)
    }

    fn index_status(&self) -> IndexRunView {
        core::index::run_status(&Config::db_path())
    }

    fn update_start(&self) -> Answer<UpdateStarted> {
        core::update::start(&Paths::from_env(), &self.cli)
    }

    fn restart(&self) -> Answer<AppRestarted> {
        let paths = Paths::from_env();
        let mut report = Vec::new();
        rhebok::restart::restart(&paths, &mut report)
            .map_err(|e| DashboardError::Conflict(format!("{e:#}")))?;
        let app = crate::relaunch::installed_app(&paths, std::env::current_exe().ok());
        let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        let pid = crate::relaunch::relaunch(&app, &args, &paths.db.with_extension("log"))
            .map_err(DashboardError::Internal)?;
        Ok(AppRestarted {
            pid,
            report: String::from_utf8_lossy(&report).into_owned(),
        })
    }

    fn install(&self) -> InstallView {
        core::install::view(&self.config_path, &self.agent_env(), &self.cli)
    }
}

pub struct RetryingReads<B> {
    inner: B,
    pause: Duration,
}

impl<B: Backend> RetryingReads<B> {
    pub fn new(inner: B) -> RetryingReads<B> {
        RetryingReads::with_pause(inner, READ_RETRY_PAUSE)
    }

    pub fn with_pause(inner: B, pause: Duration) -> RetryingReads<B> {
        RetryingReads { inner, pause }
    }

    pub fn inner(&self) -> &B {
        &self.inner
    }

    fn read<T>(&self, call: impl Fn(&B) -> Answer<T>) -> Answer<T> {
        match call(&self.inner) {
            Err(DashboardError::Unavailable(_)) => {
                std::thread::sleep(self.pause);
                call(&self.inner)
            }
            answer => answer,
        }
    }
}

impl<B: Backend> Backend for RetryingReads<B> {
    fn stats(&self) -> Answer<Stats> {
        self.read(|b| b.stats())
    }

    fn progress(&self) -> Answer<Progress> {
        self.read(|b| b.progress())
    }

    fn graph(&self) -> Answer<Graph> {
        self.read(|b| b.graph())
    }

    fn document(&self, id: String) -> Answer<Document> {
        self.read(|b| b.document(id.clone()))
    }

    fn version(&self, refresh: bool) -> Answer<Version> {
        self.read(|b| b.version(refresh))
    }

    fn search(&self, query: SearchQuery) -> Answer<SearchOutcome> {
        self.read(|b| b.search(query.clone()))
    }

    fn memories(&self) -> Memories {
        self.inner.memories()
    }

    fn memory_reach(&self, id: String) -> Answer<MemoryReach> {
        self.read(|b| b.memory_reach(id.clone()))
    }

    fn memory_save(&self, request: SaveRequest) -> Answer<SaveOutcome> {
        self.inner.memory_save(request)
    }

    fn memory_delete(&self, request: DeleteRequest) -> Answer<DeleteOutcome> {
        self.inner.memory_delete(request)
    }

    fn config_view(&self) -> Answer<ConfigView> {
        self.read(|b| b.config_view())
    }

    fn config_save(&self, request: ConfigSave) -> Answer<ConfigSaved> {
        self.inner.config_save(request)
    }

    fn config_check(&self, request: ConfigCheck) -> Answer<ConfigChecked> {
        self.inner.config_check(request)
    }

    fn set_embed_endpoint(&self, request: EmbedEndpoint) -> Answer<EmbedEndpointSaved> {
        self.inner.set_embed_endpoint(request)
    }

    fn embed_test(&self) -> EmbedTest {
        self.inner.embed_test()
    }

    fn ignore_preview(&self, pattern: String) -> Answer<IgnorePreview> {
        self.read(|b| b.ignore_preview(pattern.clone()))
    }

    fn agents(&self) -> AgentList {
        self.inner.agents()
    }

    fn agent_connect(&self, request: AgentRequest) -> Answer<AgentChanged> {
        self.inner.agent_connect(request)
    }

    fn agent_disconnect(&self, request: AgentRequest) -> Answer<AgentChanged> {
        self.inner.agent_disconnect(request)
    }

    fn index_start(&self, reindex: bool) -> Answer<IndexStarted> {
        self.inner.index_start(reindex)
    }

    fn index_status(&self) -> IndexRunView {
        self.inner.index_status()
    }

    fn update_start(&self) -> Answer<UpdateStarted> {
        self.inner.update_start()
    }

    fn restart(&self) -> Answer<AppRestarted> {
        self.inner.restart()
    }

    fn install(&self) -> InstallView {
        self.inner.install()
    }
}

pub fn describe(e: &DashboardError) -> String {
    match e {
        DashboardError::Unavailable(_) => "The index is busy — try again in a moment.".to_string(),
        other => other.to_string(),
    }
}

pub struct BackendHandle {
    backend: Arc<dyn Backend>,
    pending: Arc<AtomicUsize>,
}

impl Global for BackendHandle {}

pub fn install(cx: &mut App, backend: Arc<dyn Backend>) {
    cx.set_global(BackendHandle {
        backend,
        pending: Arc::new(AtomicUsize::new(0)),
    });
}

pub fn pending_backend_tasks(cx: &App) -> usize {
    cx.try_global::<BackendHandle>()
        .map_or(0, |h| h.pending.load(Ordering::SeqCst))
}

struct Pending(Arc<AtomicUsize>);

impl Pending {
    fn begin(count: &Arc<AtomicUsize>) -> Arc<Pending> {
        count.fetch_add(1, Ordering::SeqCst);
        Arc::new(Pending(count.clone()))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn run_backend<V: 'static, T: Send + 'static>(
    cx: &mut Context<V>,
    call: impl FnOnce(&dyn Backend) -> T + Send + 'static,
    done: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) -> Task<()> {
    let Some((backend, count)) = cx
        .try_global::<BackendHandle>()
        .map(|h| (h.backend.clone(), h.pending.clone()))
    else {
        return Task::ready(());
    };
    let pending = Pending::begin(&count);
    let calling = pending.clone();
    let work = cx.background_executor().spawn(async move {
        let value = call(backend.as_ref());
        drop(calling);
        value
    });
    cx.spawn(async move |this, cx| {
        let _pending = pending;
        let value = work.await;
        let _ = this.update(cx, |view, cx| {
            done(view, value, cx);
            cx.notify();
        });
    })
}

pub trait Reply {
    fn reply(&self) -> Self;
}

pub fn copy_error(e: &DashboardError) -> DashboardError {
    match e {
        DashboardError::BadRequest(m) => DashboardError::BadRequest(m.clone()),
        DashboardError::NotFound(m) => DashboardError::NotFound(m.clone()),
        DashboardError::Conflict(m) => DashboardError::Conflict(m.clone()),
        DashboardError::ConfigChanged { message, etag } => DashboardError::ConfigChanged {
            message: message.clone(),
            etag: etag.clone(),
        },
        DashboardError::Unprocessable(errors) => DashboardError::Unprocessable(errors.clone()),
        DashboardError::AgentRefused { kind, message } => DashboardError::AgentRefused {
            kind: *kind,
            message: message.clone(),
        },
        DashboardError::Unavailable(m) => DashboardError::Unavailable(m.clone()),
        DashboardError::VectorExtensionMissing(m) => {
            DashboardError::VectorExtensionMissing(m.clone())
        }
        DashboardError::CliMissing { fix } => DashboardError::CliMissing { fix: fix.clone() },
        DashboardError::Internal(m) => DashboardError::Internal(m.clone()),
    }
}

impl<T: Clone> Reply for Answer<T> {
    fn reply(&self) -> Self {
        match self {
            Ok(value) => Ok(value.clone()),
            Err(e) => Err(copy_error(e)),
        }
    }
}

macro_rules! reply_by_clone {
    ($($ty:ty),*) => {
        $(impl Reply for $ty {
            fn reply(&self) -> Self {
                self.clone()
            }
        })*
    };
}

reply_by_clone!(Memories, EmbedTest, AgentList, IndexRunView, InstallView);

pub struct Canned<T> {
    answer: Mutex<T>,
    next: Mutex<VecDeque<T>>,
    delay: Mutex<Option<Duration>>,
    hold: Mutex<Option<Receiver<()>>>,
    calls: AtomicUsize,
}

impl<T: Reply> Canned<T> {
    pub fn new(answer: T) -> Canned<T> {
        Canned {
            answer: Mutex::new(answer),
            next: Mutex::new(VecDeque::new()),
            delay: Mutex::new(None),
            hold: Mutex::new(None),
            calls: AtomicUsize::new(0),
        }
    }

    pub fn set(&self, answer: T) {
        *self.answer.lock().unwrap() = answer;
    }

    pub fn once(&self, answer: T) {
        self.next.lock().unwrap().push_back(answer);
    }

    pub fn delay(&self, delay: Duration) {
        *self.delay.lock().unwrap() = Some(delay);
    }

    pub fn hold(&self) -> Sender<()> {
        let (release, held) = channel();
        *self.hold.lock().unwrap() = Some(held);
        release
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn answer(&self) -> T {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let delay = *self.delay.lock().unwrap();
        if let Some(delay) = delay {
            std::thread::sleep(delay);
        }
        let held = self.hold.lock().unwrap().take();
        if let Some(held) = held {
            let _ = held.recv_timeout(HOLD_LIMIT);
        }
        let next = self.next.lock().unwrap().pop_front();
        next.unwrap_or_else(|| self.answer.lock().unwrap().reply())
    }
}

fn unset<T>(method: &str) -> Answer<T> {
    Err(DashboardError::Internal(format!(
        "FixtureBackend has no answer for {method}"
    )))
}

pub struct FixtureBackend {
    pub stats: Canned<Answer<Stats>>,
    pub progress: Canned<Answer<Progress>>,
    pub graph: Canned<Answer<Graph>>,
    pub document: Canned<Answer<Document>>,
    pub version: Canned<Answer<Version>>,
    pub search: Canned<Answer<SearchOutcome>>,
    pub memories: Canned<Memories>,
    pub memory_reach: Canned<Answer<MemoryReach>>,
    pub memory_save: Canned<Answer<SaveOutcome>>,
    pub memory_delete: Canned<Answer<DeleteOutcome>>,
    pub config_view: Canned<Answer<ConfigView>>,
    pub config_save: Canned<Answer<ConfigSaved>>,
    pub config_check: Canned<Answer<ConfigChecked>>,
    pub set_embed_endpoint: Canned<Answer<EmbedEndpointSaved>>,
    pub embed_test: Canned<EmbedTest>,
    pub ignore_preview: Canned<Answer<IgnorePreview>>,
    pub agents: Canned<AgentList>,
    pub agent_connect: Canned<Answer<AgentChanged>>,
    pub agent_disconnect: Canned<Answer<AgentChanged>>,
    pub index_start: Canned<Answer<IndexStarted>>,
    pub index_status: Canned<IndexRunView>,
    pub update_start: Canned<Answer<UpdateStarted>>,
    pub restart: Canned<Answer<AppRestarted>>,
    pub install: Canned<InstallView>,
    pub requests: Mutex<Vec<String>>,
    pub journal: Mutex<Vec<String>>,
}

impl Default for FixtureBackend {
    fn default() -> FixtureBackend {
        FixtureBackend {
            stats: Canned::new(unset("stats")),
            progress: Canned::new(Ok(Progress::Idle { idle: true })),
            graph: Canned::new(unset("graph")),
            document: Canned::new(unset("document")),
            version: Canned::new(unset("version")),
            search: Canned::new(unset("search")),
            memories: Canned::new(Memories {
                memories: Vec::new(),
                unavailable: None,
            }),
            memory_reach: Canned::new(unset("memory_reach")),
            memory_save: Canned::new(unset("memory_save")),
            memory_delete: Canned::new(unset("memory_delete")),
            config_view: Canned::new(unset("config_view")),
            config_save: Canned::new(unset("config_save")),
            config_check: Canned::new(unset("config_check")),
            set_embed_endpoint: Canned::new(unset("set_embed_endpoint")),
            embed_test: Canned::new(EmbedTest {
                ok: false,
                latency_ms: 0,
                dimensions: None,
                backend: "ollama",
                url: None,
                model: None,
                error: Some("FixtureBackend has no answer for embed_test".to_string()),
            }),
            ignore_preview: Canned::new(unset("ignore_preview")),
            agents: Canned::new(AgentList(serde_json::json!({ "agents": [] }))),
            agent_connect: Canned::new(unset("agent_connect")),
            agent_disconnect: Canned::new(unset("agent_disconnect")),
            index_start: Canned::new(unset("index_start")),
            index_status: Canned::new(IndexRunView { run: None }),
            update_start: Canned::new(unset("update_start")),
            restart: Canned::new(unset("restart")),
            install: Canned::new(InstallView {
                root: PathBuf::new(),
                binary: PathBuf::new(),
                exe: PathBuf::new(),
                checks: Vec::new(),
                config: InstallConfig {
                    path: PathBuf::new(),
                    errors: Vec::new(),
                },
                embed: EmbedView {
                    backend: "ollama",
                    url: None,
                    model: None,
                },
            }),
            requests: Mutex::new(Vec::new()),
            journal: Mutex::new(Vec::new()),
        }
    }
}

impl FixtureBackend {
    fn record(&self, request: String) {
        self.requests.lock().unwrap().push(request);
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    pub fn note(&self, event: impl Into<String>) {
        self.journal.lock().unwrap().push(event.into());
    }

    pub fn journal(&self) -> Vec<String> {
        self.journal.lock().unwrap().clone()
    }

    fn reply<T: Reply>(&self, method: &str, canned: &Canned<T>) -> T {
        let answer = canned.answer();
        self.note(format!("{method} returned"));
        answer
    }
}

impl Backend for FixtureBackend {
    fn stats(&self) -> Answer<Stats> {
        self.reply("stats", &self.stats)
    }

    fn progress(&self) -> Answer<Progress> {
        self.reply("progress", &self.progress)
    }

    fn graph(&self) -> Answer<Graph> {
        self.reply("graph", &self.graph)
    }

    fn document(&self, id: String) -> Answer<Document> {
        self.record(format!("document {id}"));
        self.reply("document", &self.document)
    }

    fn version(&self, refresh: bool) -> Answer<Version> {
        self.record(format!("version refresh={refresh}"));
        self.reply("version", &self.version)
    }

    fn search(&self, query: SearchQuery) -> Answer<SearchOutcome> {
        self.record(format!("search {:?} tier={:?}", query.q, query.tier));
        self.reply("search", &self.search)
    }

    fn memories(&self) -> Memories {
        self.reply("memories", &self.memories)
    }

    fn memory_reach(&self, id: String) -> Answer<MemoryReach> {
        self.record(format!("memory_reach {id}"));
        self.reply("memory_reach", &self.memory_reach)
    }

    fn memory_save(&self, request: SaveRequest) -> Answer<SaveOutcome> {
        self.record(format!("memory_save {request:?}"));
        self.reply("memory_save", &self.memory_save)
    }

    fn memory_delete(&self, request: DeleteRequest) -> Answer<DeleteOutcome> {
        self.record(format!("memory_delete {request:?}"));
        self.reply("memory_delete", &self.memory_delete)
    }

    fn config_view(&self) -> Answer<ConfigView> {
        self.reply("config_view", &self.config_view)
    }

    fn config_save(&self, request: ConfigSave) -> Answer<ConfigSaved> {
        self.record(format!("config_save {request:?}"));
        self.reply("config_save", &self.config_save)
    }

    fn config_check(&self, request: ConfigCheck) -> Answer<ConfigChecked> {
        self.record(format!("config_check {request:?}"));
        self.reply("config_check", &self.config_check)
    }

    fn set_embed_endpoint(&self, request: EmbedEndpoint) -> Answer<EmbedEndpointSaved> {
        self.record(format!("set_embed_endpoint {request:?}"));
        self.reply("set_embed_endpoint", &self.set_embed_endpoint)
    }

    fn embed_test(&self) -> EmbedTest {
        self.reply("embed_test", &self.embed_test)
    }

    fn ignore_preview(&self, pattern: String) -> Answer<IgnorePreview> {
        self.record(format!("ignore_preview {pattern}"));
        self.reply("ignore_preview", &self.ignore_preview)
    }

    fn agents(&self) -> AgentList {
        self.reply("agents", &self.agents)
    }

    fn agent_connect(&self, request: AgentRequest) -> Answer<AgentChanged> {
        self.record(format!("agent_connect {request:?}"));
        self.reply("agent_connect", &self.agent_connect)
    }

    fn agent_disconnect(&self, request: AgentRequest) -> Answer<AgentChanged> {
        self.record(format!("agent_disconnect {request:?}"));
        self.reply("agent_disconnect", &self.agent_disconnect)
    }

    fn index_start(&self, reindex: bool) -> Answer<IndexStarted> {
        self.record(format!("index_start reindex={reindex}"));
        self.reply("index_start", &self.index_start)
    }

    fn index_status(&self) -> IndexRunView {
        self.reply("index_status", &self.index_status)
    }

    fn update_start(&self) -> Answer<UpdateStarted> {
        self.record("update_start".to_string());
        self.reply("update_start", &self.update_start)
    }

    fn restart(&self) -> Answer<AppRestarted> {
        self.record("restart".to_string());
        self.reply("restart", &self.restart)
    }

    fn install(&self) -> InstallView {
        self.reply("install", &self.install)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use gpui_kit::{AppContext, Context, Entity, TestAppContext};
    use rhebok::dashboard::core::stats::Progress;
    use rhebok::dashboard::core::DashboardError;
    use serde_json::json;

    use super::{
        describe, pending_backend_tasks, run_backend, Answer, Backend, FixtureBackend, Pending,
        RetryingReads, READ_RETRY_PAUSE,
    };

    struct Probe;

    #[gpui_kit::test]
    fn the_pending_count_follows_each_backend_task_to_its_end(cx: &mut TestAppContext) {
        cx.update(|cx| super::install(cx, Arc::new(FixtureBackend::default())));
        let probe: Entity<Probe> = cx.new(|_| Probe);
        let pending = |cx: &mut TestAppContext| cx.update(|cx| pending_backend_tasks(cx));
        assert_eq!(pending(cx), 0);

        let task = probe.update(cx, |_, cx: &mut Context<Probe>| {
            run_backend(cx, |backend| backend.stats(), |_, _, _| {})
        });
        assert_eq!(pending(cx), 1);
        cx.run_until_parked();
        assert_eq!(pending(cx), 0);
        drop(task);

        let dropped = probe.update(cx, |_, cx: &mut Context<Probe>| {
            run_backend(cx, |backend| backend.stats(), |_, _, _| {})
        });
        assert_eq!(pending(cx), 1);
        drop(dropped);
        cx.run_until_parked();
        assert_eq!(pending(cx), 0);
    }

    #[gpui_kit::test]
    fn each_backend_counts_only_its_own_tasks(cx: &mut TestAppContext) {
        assert_eq!(cx.update(|cx| pending_backend_tasks(cx)), 0);
        cx.update(|cx| super::install(cx, Arc::new(FixtureBackend::default())));
        let probe: Entity<Probe> = cx.new(|_| Probe);
        let _task = probe.update(cx, |_, cx: &mut Context<Probe>| {
            run_backend(cx, |backend| backend.stats(), |_, _, _| {})
        });
        assert_eq!(cx.update(|cx| pending_backend_tasks(cx)), 1);
        cx.update(|cx| super::install(cx, Arc::new(FixtureBackend::default())));
        assert_eq!(cx.update(|cx| pending_backend_tasks(cx)), 0);
    }

    #[test]
    fn a_task_stays_pending_until_its_background_call_ends_even_once_dropped() {
        let count = Arc::new(AtomicUsize::new(0));
        let foreground = Pending::begin(&count);
        let background = foreground.clone();
        let (release, held) = channel::<()>();
        let call = std::thread::spawn(move || {
            held.recv().unwrap();
            drop(background);
        });
        drop(foreground);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        release.send(()).unwrap();
        call.join().unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    fn busy<T>() -> Answer<T> {
        Err(DashboardError::Unavailable("the store is mid-swap".into()))
    }

    fn retrying(pause: Duration) -> RetryingReads<FixtureBackend> {
        let fixture = FixtureBackend::default();
        fixture.progress.set(Ok(Progress::Idle { idle: true }));
        RetryingReads::with_pause(fixture, pause)
    }

    #[test]
    fn a_read_waits_the_webs_half_second_before_its_retry() {
        assert_eq!(READ_RETRY_PAUSE, Duration::from_millis(500));
        let backend = RetryingReads::new(FixtureBackend::default());
        backend.inner().progress.once(busy());
        let started = Instant::now();
        assert!(backend.progress().is_ok());
        assert!(started.elapsed() >= READ_RETRY_PAUSE);
    }

    #[test]
    fn a_read_that_is_unavailable_once_succeeds_on_the_retry() {
        let backend = retrying(Duration::from_millis(20));
        backend.inner().progress.once(busy());
        assert!(matches!(
            backend.progress(),
            Ok(Progress::Idle { idle: true })
        ));
        assert_eq!(backend.inner().progress.calls(), 2);
    }

    #[test]
    fn a_read_that_is_unavailable_twice_fails() {
        let backend = retrying(Duration::from_millis(20));
        backend.inner().progress.once(busy());
        backend.inner().progress.once(busy());
        assert!(matches!(
            backend.progress(),
            Err(DashboardError::Unavailable(_))
        ));
        assert_eq!(backend.inner().progress.calls(), 2);
    }

    #[test]
    fn a_read_is_not_retried_for_any_other_error() {
        let backend = retrying(Duration::from_millis(20));
        backend
            .inner()
            .progress
            .once(Err(DashboardError::Internal("broken".into())));
        assert!(matches!(
            backend.progress(),
            Err(DashboardError::Internal(_))
        ));
        assert_eq!(backend.inner().progress.calls(), 1);
    }

    #[test]
    fn every_read_the_web_fetches_with_get_is_retried() {
        let backend = retrying(Duration::from_millis(1));
        let f = backend.inner();
        f.stats.once(busy());
        f.graph.once(busy());
        f.document.once(busy());
        f.version.once(busy());
        f.search.once(busy());
        f.memory_reach.once(busy());
        f.config_view.once(busy());
        f.ignore_preview.once(busy());
        let _ = backend.stats();
        let _ = backend.graph();
        let _ = backend.document("d".into());
        let _ = backend.version(false);
        let _ = backend.search(Default::default());
        let _ = backend.memory_reach("m".into());
        let _ = backend.config_view();
        let _ = backend.ignore_preview("*.tmp".into());
        let calls = [
            f.stats.calls(),
            f.graph.calls(),
            f.document.calls(),
            f.version.calls(),
            f.search.calls(),
            f.memory_reach.calls(),
            f.config_view.calls(),
            f.ignore_preview.calls(),
        ];
        assert_eq!(calls, [2; 8]);
    }

    macro_rules! request {
        ($value:tt) => {
            serde_json::from_value(json!($value)).unwrap()
        };
    }

    #[test]
    fn a_write_is_never_retried() {
        let backend = retrying(Duration::from_millis(1));
        let f = backend.inner();
        f.memory_save.set(busy());
        f.memory_delete.set(busy());
        f.config_save.set(busy());
        f.config_check.set(busy());
        f.set_embed_endpoint.set(busy());
        f.agent_connect.set(busy());
        f.agent_disconnect.set(busy());
        f.index_start.set(busy());
        f.update_start.set(busy());
        let _ = backend.memory_save(request!({"id": null, "kind": "lesson", "text": "t", "scope": "global", "confidence": null}));
        let _ = backend.memory_delete(request!({"id": "m"}));
        let _ = backend.config_save(request!({"etag": "e"}));
        let _ = backend.config_check(request!({}));
        let _ = backend.set_embed_endpoint(request!({}));
        let _ = backend.agent_connect(request!({"id": "cursor"}));
        let _ = backend.agent_disconnect(request!({"id": "cursor"}));
        let _ = backend.index_start(false);
        let _ = backend.update_start();
        let calls = [
            f.memory_save.calls(),
            f.memory_delete.calls(),
            f.config_save.calls(),
            f.config_check.calls(),
            f.set_embed_endpoint.calls(),
            f.agent_connect.calls(),
            f.agent_disconnect.calls(),
            f.index_start.calls(),
            f.update_start.calls(),
        ];
        assert_eq!(calls, [1; 9]);
    }

    #[test]
    fn describe_maps_unavailable_to_the_busy_text() {
        assert_eq!(
            describe(&DashboardError::Unavailable("store open failed".into())),
            "The index is busy — try again in a moment."
        );
        assert_eq!(
            describe(&DashboardError::Conflict(
                "an index is already running".into()
            )),
            "an index is already running"
        );
        assert_eq!(
            describe(&DashboardError::CliMissing { fix: "x".into() }),
            "the rhebok CLI was not found"
        );
    }
}
