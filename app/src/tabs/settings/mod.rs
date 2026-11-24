pub mod advanced;
pub mod documents;
pub mod embedding;
pub mod fields;
pub mod list_editor;
pub mod memory;
pub mod nav;
pub mod pending;
pub mod raw_toml;
pub mod retrieval;
pub mod save_bar;
#[cfg(test)]
mod section_tests;
pub mod sources;
#[cfg(test)]
mod tests;
pub mod updates;

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::base::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::config::check::FieldError;
use rhebok::dashboard::core::config::{ConfigChecked, EmbedEndpoint};
use rhebok::dashboard::core::DashboardError;
use serde_json::Value as Json;

use crate::backend::{describe as describe_error, run_backend};
use crate::shell::Shell;
use crate::tabs::health::index_panel::{IndexFinished, IndexState, IndexStatus};
use crate::theme::{css_line_box, tokens, Face, Tokens, Type, MONO, SANS, SM_FACE, XS_FACE};
use crate::widgets::banner;
use crate::widgets::css_box::css_box;
use crate::widgets::text_input::{input_state, textarea_state};
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{baseline, button, exact_text, text_button};
use embedding::{EmbedLocal, Mode};
use fields::{
    code_line, number_text, parse_keep_alive, parse_number, section_marker, Ctx, FieldBounds,
    InputKind, InputSpec, Part,
};
use list_editor::{string_list, ListState};
use nav::{nav, section_of, NavRow, Section, SECTIONS};
use pending::{merge, ConfigState, Pending};
use raw_toml::{RawLocal, RawView};
use save_bar::{save_bar, SaveBar};

pub const CHECK_DEBOUNCE: Duration = Duration::from_millis(300);
pub const RING_FOR: Duration = Duration::from_millis(1600);
pub const FOCUS_POLL: Duration = Duration::from_millis(16);
pub const FOCUS_TRIES: usize = 600;

pub const HOOK_NOTE: &str = "The prompt hook reads config.toml on every prompt, so saved changes apply from your next prompt; nothing needs a restart.";
pub const TITLE_NOTE: &str =
    "Saved changes reach the prompt hook on its next prompt, with no restart.";
pub const SAVING: &str = "Saving config.toml. The form is locked until the server answers.";
pub const UNPARSEABLE: &str = "config.toml does not parse, so rhebok is running on defaults and the form shows them. Fix the syntax error by hand; until then nothing can be saved from here.";
pub const FILE_ERRORS_HINT: &str =
    "These were in the file before this page opened. They do not block saving other settings.";
pub const LOADING: &str = "Loading…";

const H2_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 20., 28.);
const BUTTON_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 12., 16.);
const DISMISS_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 12., 16.);
const REINDEX_CODE_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 14., 20.);

pub fn conflict_text(dirty: usize) -> String {
    format!(
        "config.toml changed on disk since this page read it, so nothing was saved. Reload to see the new file and lose your {dirty} unsaved change{}, or keep editing: your changes stay and the next save applies them on top of the new file.",
        if dirty == 1 { "" } else { "s" }
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub text: String,
    pub warn: bool,
    pub reindex: bool,
}

pub struct Settings {
    shell: WeakEntity<Shell>,
    state: Option<ConfigState>,
    load_err: Option<String>,
    pending: Pending,
    checked: Option<(Pending, ConfigChecked)>,
    server_errors: Vec<FieldError>,
    section: Section,
    preview: bool,
    saving: bool,
    conflict: bool,
    notice: Option<Notice>,
    focus: Option<String>,
    ring: Option<String>,
    scroll: ScrollHandle,
    bounds: FieldBounds,
    inputs: HashMap<String, Entity<InputState>>,
    synced: HashMap<String, Option<Json>>,
    lists: HashMap<String, ListState>,
    embed: EmbedLocal,
    endpoint_busy: bool,
    endpoint_generation: u64,
    draft_resets: HashMap<String, String>,
    raw: RawLocal,
    raw_area: Option<Entity<TextareaState>>,
    raw_synced: Option<String>,
    raw_events: Option<Subscription>,
    index: Entity<IndexStatus>,
    load_task: Option<Task<()>>,
    check_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
    focus_task: Option<Task<()>>,
    ring_task: Option<Task<()>>,
    endpoint_task: Option<Task<()>>,
    test_task: Option<Task<()>>,
    input_events: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl Settings {
    pub fn new(shell: WeakEntity<Shell>, cx: &mut Context<Settings>) -> Settings {
        let index = cx.new(|_| IndexStatus::new("settings-index-status"));
        let mut subscriptions = vec![
            cx.subscribe(&index, |settings, _, _: &IndexFinished, cx| {
                if let Some(shell) = settings.shell.upgrade() {
                    shell.update(cx, |shell, cx| shell.index_finished(cx));
                }
            }),
            cx.observe(&index, |_, _, cx| cx.notify()),
        ];
        if let Some(owner) = shell.upgrade() {
            subscriptions.push(cx.observe(&owner, |_, _, cx| cx.notify()));
        }
        let mut settings = Settings {
            shell,
            state: None,
            load_err: None,
            pending: Pending::default(),
            checked: None,
            server_errors: Vec::new(),
            section: Section::default(),
            preview: false,
            saving: false,
            conflict: false,
            notice: None,
            focus: None,
            ring: None,
            scroll: ScrollHandle::new(),
            bounds: FieldBounds::default(),
            inputs: HashMap::new(),
            synced: HashMap::new(),
            lists: HashMap::new(),
            embed: EmbedLocal::default(),
            endpoint_busy: false,
            endpoint_generation: 0,
            draft_resets: HashMap::new(),
            raw: RawLocal::default(),
            raw_area: None,
            raw_synced: None,
            raw_events: None,
            index,
            load_task: None,
            check_task: None,
            save_task: None,
            focus_task: None,
            ring_task: None,
            endpoint_task: None,
            test_task: None,
            input_events: Vec::new(),
            _subscriptions: subscriptions,
        };
        settings.load(false, cx);
        settings
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn pending(&self) -> &Pending {
        &self.pending
    }

    pub fn ring(&self) -> Option<&str> {
        self.ring.as_deref()
    }

    pub fn etag(&self) -> Option<&str> {
        self.state.as_ref().map(ConfigState::etag)
    }

    pub fn preview_open(&self) -> bool {
        self.preview
    }

    pub fn conflict(&self) -> bool {
        self.conflict
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn index_status(&self) -> &Entity<IndexStatus> {
        &self.index
    }

    pub fn raw_local(&self) -> &RawLocal {
        &self.raw
    }

    pub fn raw_area(&self) -> Option<&Entity<TextareaState>> {
        self.raw_area.as_ref()
    }

    pub fn raw_mirror(&self) -> Option<String> {
        let state = self.state.as_ref().filter(|s| !s.unparseable())?;
        Some(raw_toml::mirror(state.file(), &self.pending))
    }

    pub fn raw_text(&self) -> Option<String> {
        self.raw.draft.clone().or_else(|| self.raw_mirror())
    }

    pub fn raw_edited(&mut self, text: String, cx: &mut Context<Settings>) {
        self.raw.draft = Some(text);
        cx.notify();
    }

    pub fn apply_raw(&mut self, cx: &mut Context<Settings>) {
        if self.locked() {
            return;
        }
        let Some(draft) = self.raw.draft.clone() else {
            return;
        };
        let Some(state) = self.state.as_ref().filter(|s| !s.unparseable()) else {
            return;
        };
        match raw_toml::parse(&draft) {
            Err(problem) => self.raw.problem = Some(problem),
            Ok(document) => {
                self.raw.problem = None;
                self.pending = Pending::from_document(state.file(), &document);
                self.preview = false;
                self.raw.draft = None;
                self.raw_synced = None;
                self.schedule_check(cx);
                for path in self.pending.paths() {
                    self.revalidate(&path, cx);
                }
            }
        }
        cx.notify();
    }

    pub fn revert_raw(&mut self, cx: &mut Context<Settings>) {
        self.raw.draft = None;
        self.raw.problem = None;
        self.raw_synced = None;
        cx.notify();
    }

    pub fn copy_raw(&mut self, cx: &mut Context<Settings>) {
        let text = self.raw_text().unwrap_or_default();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.raw.copied = true;
        let generation = self.raw.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(raw_toml::COPIED_FOR).await;
            let _ = this.update(cx, |settings, cx| {
                if settings.raw.generation == generation {
                    settings.raw.copied = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn leave_advanced(&mut self) {
        self.raw.reset();
        self.raw_area = None;
        self.raw_synced = None;
        self.raw_events = None;
    }

    fn ensure_raw(&mut self, window: &mut Window, cx: &mut Context<Settings>) {
        if self.section != Section::Advanced {
            return;
        }
        let Some(mirror) = self.raw_mirror() else {
            return;
        };
        let t = *tokens(cx);
        let area = match &self.raw_area {
            Some(area) => area.clone(),
            None => {
                let area = textarea_state(&mirror, raw_toml::rows(&mirror), &t, window, cx);
                self.raw_events =
                    Some(
                        cx.subscribe_in(&area, window, |settings, state, event, _, cx| {
                            if let InputEvent::Change = event {
                                let text = state.read(cx).value().to_string();
                                settings.raw_edited(text, cx);
                            }
                        }),
                    );
                self.raw_area = Some(area.clone());
                area
            }
        };
        let locked = self.locked();
        let follow = self.raw.draft.is_none() && self.raw_synced.as_ref() != Some(&mirror);
        if follow {
            self.raw_synced = Some(mirror.clone());
        }
        area.update(cx, |state, cx| {
            if follow && state.value().as_ref() != mirror {
                state.set_value(mirror, window, cx);
            }
            state.set_readonly(locked, cx);
        });
    }

    pub fn input(&self, key: &str) -> Option<&Entity<InputState>> {
        self.inputs.get(key)
    }

    pub fn inputs_hold(&self, text: &str, cx: &App) -> Vec<String> {
        self.inputs
            .iter()
            .filter(|(_, input)| input.read(cx).value().contains(text))
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn locked(&self) -> bool {
        self.saving || self.endpoint_busy
    }

    pub fn embed_local(&self) -> &EmbedLocal {
        &self.embed
    }

    fn fresh(&self) -> Option<&ConfigChecked> {
        self.checked
            .as_ref()
            .filter(|(sent, _)| *sent == self.pending)
            .map(|(_, checked)| checked)
    }

    pub fn checking(&self) -> bool {
        !self.pending.is_empty() && self.fresh().is_none()
    }

    pub fn blocking(&self) -> Vec<FieldError> {
        let fresh = self.fresh().map(|c| c.blocking.clone()).unwrap_or_default();
        merge(&fresh, &self.server_errors)
    }

    pub fn errors(&self) -> Vec<FieldError> {
        let Some(state) = &self.state else {
            return Vec::new();
        };
        let base = match self.fresh() {
            Some(checked) if !self.pending.is_empty() => checked.errors.clone(),
            _ => state.errors(),
        };
        merge(&base, &self.server_errors)
    }

    pub fn can_save(&self) -> bool {
        self.state.as_ref().is_some_and(|s| !s.unparseable())
            && !self.locked()
            && !self.checking()
            && self.blocking().is_empty()
    }

    pub fn load(&mut self, keep_pending: bool, cx: &mut Context<Settings>) {
        self.load_err = None;
        self.load_task = Some(run_backend(
            cx,
            |backend| backend.config_view(),
            move |settings: &mut Settings, result, cx| match result {
                Ok(view) => {
                    settings.state = Some(ConfigState::new(view.0));
                    settings.server_errors.clear();
                    settings.poll_focus(cx);
                    if !keep_pending {
                        settings.pending = Pending::default();
                        settings.preview = false;
                        settings.schedule_check(cx);
                    }
                }
                Err(e) => {
                    settings.load_err = Some(describe_error(&e));
                    settings.focus_task = None;
                }
            },
        ));
    }

    pub fn set(&mut self, path: &str, value: Option<toml::Value>, cx: &mut Context<Settings>) {
        if self.locked() {
            return;
        }
        let Some(state) = &self.state else {
            return;
        };
        self.preview = false;
        let before = self.pending.clone();
        self.pending.change(state, path, value);
        if self.pending != before {
            self.schedule_check(cx);
            self.revalidate(path, cx);
        }
        cx.notify();
    }

    fn schedule_check(&mut self, cx: &mut Context<Settings>) {
        self.server_errors.clear();
        if self.pending.is_empty() {
            self.checked = None;
            self.check_task = None;
            return;
        }
        let sent = self.pending.clone();
        self.check_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHECK_DEBOUNCE).await;
            let request = sent.to_check();
            let call = this.update(cx, move |_, cx| {
                run_backend(
                    cx,
                    move |backend| backend.config_check(request),
                    move |settings: &mut Settings, result, _| {
                        let checked = result.unwrap_or(ConfigChecked {
                            errors: Vec::new(),
                            blocking: Vec::new(),
                        });
                        settings.checked = Some((sent, checked));
                    },
                )
            });
            if let Ok(call) = call {
                call.await;
            }
        }));
    }

    pub fn discard(&mut self, cx: &mut Context<Settings>) {
        if self.locked() {
            return;
        }
        self.pending = Pending::default();
        self.preview = false;
        self.schedule_check(cx);
        cx.notify();
    }

    pub fn open_preview(&mut self, cx: &mut Context<Settings>) {
        if self.can_save() {
            self.preview = true;
            cx.notify();
        }
    }

    pub fn close_preview(&mut self, cx: &mut Context<Settings>) {
        if !self.locked() {
            self.preview = false;
            cx.notify();
        }
    }

    pub fn save(&mut self, cx: &mut Context<Settings>) {
        if self.locked() || !self.can_save() {
            return;
        }
        let Some(state) = &self.state else {
            return;
        };
        let sent = self.pending.clone();
        let request = sent.to_save(state.etag());
        self.saving = true;
        self.notice = None;
        self.save_task = Some(run_backend(
            cx,
            move |backend| backend.config_save(request),
            move |settings: &mut Settings, result, cx| {
                settings.saving = false;
                match result {
                    Ok(view) => {
                        let state = ConfigState::new(view.0);
                        let path = state.path().to_string();
                        settings.state = Some(state);
                        settings.pending = Pending::default();
                        settings.preview = false;
                        settings.conflict = false;
                        settings.schedule_check(cx);
                        settings.notice = Some(Notice {
                            text: format!("Saved to {path}. {HOOK_NOTE}"),
                            warn: false,
                            reindex: sent.needs_reindex(),
                        });
                        if let Some(shell) = settings.shell.upgrade() {
                            shell.update(cx, |shell, cx| shell.refresh_stats(cx));
                        }
                    }
                    Err(DashboardError::ConfigChanged { .. }) => {
                        settings.preview = false;
                        settings.conflict = true;
                    }
                    Err(DashboardError::Unprocessable(errors)) => {
                        let text = DashboardError::Unprocessable(errors.clone()).to_string();
                        settings.server_errors = errors;
                        settings.preview = false;
                        settings.notice = Some(Notice {
                            text: format!("Nothing was saved. {text}"),
                            warn: true,
                            reindex: false,
                        });
                    }
                    Err(e) => {
                        settings.notice = Some(Notice {
                            text: format!("Nothing was saved: {}", describe_error(&e)),
                            warn: true,
                            reindex: false,
                        });
                    }
                }
            },
        ));
        cx.notify();
    }

    pub fn reload_discarding(&mut self, cx: &mut Context<Settings>) {
        self.conflict = false;
        self.load(false, cx);
        cx.notify();
    }

    pub fn keep_editing(&mut self, cx: &mut Context<Settings>) {
        self.conflict = false;
        self.load(true, cx);
        cx.notify();
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Settings>) {
        self.notice = None;
        cx.notify();
    }

    pub fn pick_section(&mut self, section: Section, cx: &mut Context<Settings>) {
        if section != self.section {
            if self.section == Section::Embedding {
                self.leave_embedding();
            }
            if self.section == Section::Advanced {
                self.leave_advanced();
            }
            self.section = section;
            if self.focus.is_none() {
                self.scroll.set_offset(Point::default());
            }
        }
        cx.notify();
    }

    pub fn jump(&mut self, path: &str, cx: &mut Context<Settings>) {
        self.focus_field(path, cx);
    }

    pub fn focus_field(&mut self, key: &str, cx: &mut Context<Settings>) {
        let section = section_of(key);
        if self.section == Section::Embedding && section != Section::Embedding {
            self.leave_embedding();
        }
        if self.section == Section::Advanced && section != Section::Advanced {
            self.leave_advanced();
        }
        self.section = section;
        self.focus = Some(key.to_string());
        self.poll_focus(cx);
        cx.notify();
    }

    fn waiting_to_focus(&self) -> bool {
        self.focus.is_some() && self.load_err.is_none()
    }

    fn poll_focus(&mut self, cx: &mut Context<Settings>) {
        if !self.waiting_to_focus() {
            self.focus_task = None;
            return;
        }
        self.focus_task = Some(cx.spawn(async move |this, cx| {
            for _ in 0..FOCUS_TRIES {
                cx.background_executor().timer(FOCUS_POLL).await;
                let waiting = this.update(cx, |settings, cx| {
                    let waiting = settings.waiting_to_focus();
                    if waiting {
                        cx.notify();
                    }
                    waiting
                });
                if !matches!(waiting, Ok(true)) {
                    break;
                }
            }
        }));
    }

    fn resolve_focus(&mut self, cx: &mut Context<Settings>) {
        let Some(key) = self.focus.clone() else {
            return;
        };
        if self.state.is_none() {
            return;
        }
        let found = {
            let map = self.bounds.borrow();
            if map.section != Some(self.section) {
                return;
            }
            let mut parts: Vec<&str> = key.split('.').collect();
            let mut found = None;
            while !parts.is_empty() && found.is_none() {
                let candidate = parts.join(".");
                if let Some(at) = map.fields.get(&candidate) {
                    found = Some((candidate, *at));
                }
                parts.pop();
            }
            if found.is_none() && self.section == Section::Advanced {
                found = map.fields.get("raw").map(|at| ("raw".to_string(), *at));
            }
            found
        };
        let Some((path, (painted, offset_then))) = found else {
            if self.section != Section::Advanced {
                self.section = Section::Advanced;
            } else {
                self.focus = None;
            }
            return;
        };
        let pane = self.scroll.bounds();
        let top = painted.top() - offset_then - pane.top();
        let target = top - (pane.size.height - painted.size.height) / 2.;
        let limit = self.scroll.max_offset().y.max(px(0.));
        let y = target.max(px(0.)).min(limit);
        self.scroll.set_offset(point(px(0.), -y));
        self.focus = None;
        self.ring = Some(path);
        self.ring_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RING_FOR).await;
            let _ = this.update(cx, |settings, cx| {
                settings.ring = None;
                cx.notify();
            });
        }));
    }

    fn input_specs(&self) -> Vec<InputSpec> {
        let Some(state) = &self.state else {
            return Vec::new();
        };
        match self.section {
            Section::Sources => sources::inputs(),
            Section::Retrieval => retrieval::inputs(),
            Section::Embedding => embedding::inputs(state, &self.embed),
            Section::Memory => memory::inputs(),
            Section::Documents => documents::inputs(),
            Section::Updates => updates::inputs(),
            Section::Advanced => advanced::inputs(state),
        }
    }

    fn reset_drafts(&mut self) {
        let Some(state) = &self.state else {
            return;
        };
        for key in [embedding::URL, embedding::MODEL, embedding::TOKEN] {
            self.draft_resets
                .insert(key.to_string(), embedding::draft_initial(key, state));
        }
    }

    fn leave_embedding(&mut self) {
        let seen_backend = self.embed.seen_backend;
        let seen_endpoint = self.embed.seen_endpoint.clone();
        self.embed = EmbedLocal {
            seen_backend,
            seen_endpoint,
            ..EmbedLocal::default()
        };
        self.test_task = None;
        self.endpoint_generation += 1;
        self.reset_drafts();
    }

    fn sync_embedding(&mut self) {
        let Some(state) = &self.state else {
            return;
        };
        let backend = Mode::of(state);
        if self.embed.seen_backend != Some(backend) {
            self.embed.seen_backend = Some(backend);
            if self.embed.mode.is_some_and(|m| m != backend) {
                self.embed.forget_remote();
            }
            self.embed.mode = None;
        }
        let endpoint = (state.endpoint("url"), state.endpoint("model"));
        if self.embed.seen_endpoint.as_ref() != Some(&endpoint) {
            self.embed.seen_endpoint = Some(endpoint.clone());
            self.draft_resets
                .insert(embedding::URL.to_string(), endpoint.0.unwrap_or_default());
            self.draft_resets
                .insert(embedding::MODEL.to_string(), endpoint.1.unwrap_or_default());
        }
    }

    pub fn set_embed_mode(&mut self, mode: Mode, cx: &mut Context<Settings>) {
        let Some(state) = &self.state else {
            return;
        };
        let before = self.embed.mode(state);
        if before == mode {
            return;
        }
        self.embed.forget_remote();
        self.endpoint_generation += 1;
        if before == Mode::Remote {
            self.reset_drafts();
        }
        self.embed.mode = Some(mode);
        cx.notify();
    }

    pub fn open_token(&mut self, cx: &mut Context<Settings>) {
        if self.locked() {
            return;
        }
        self.embed.replacing = true;
        cx.notify();
    }

    pub fn cancel_token(&mut self, cx: &mut Context<Settings>) {
        self.embed.replacing = false;
        self.draft_resets
            .insert(embedding::TOKEN.to_string(), String::new());
        cx.notify();
    }

    pub fn clear_token(&mut self, cx: &mut Context<Settings>) {
        self.submit_endpoint(
            EmbedEndpoint {
                url: None,
                model: None,
                token: Some(None),
            },
            embedding::TOKEN_REMOVED,
            cx,
        );
    }

    fn draft(&self, key: &str, cx: &App) -> String {
        self.inputs
            .get(key)
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default()
    }

    pub fn save_endpoint(&mut self, cx: &mut Context<Settings>) {
        let Some(state) = &self.state else {
            return;
        };
        let changed = |key: &str, field: &str, draft: String| {
            let trimmed = draft.trim().to_string();
            let saved = state.endpoint(field).unwrap_or_default();
            (state.override_for(key).is_none() && trimmed != saved)
                .then(|| (!trimmed.is_empty()).then_some(trimmed))
        };
        let url = changed(embedding::URL, "url", self.draft(embedding::URL, cx));
        let model = changed(embedding::MODEL, "model", self.draft(embedding::MODEL, cx));
        let token = self.draft(embedding::TOKEN, cx);
        let token = (self.embed.replacing && !token.is_empty()).then_some(Some(token));
        self.submit_endpoint(EmbedEndpoint { url, model, token }, embedding::SAVED, cx);
    }

    fn submit_endpoint(
        &mut self,
        change: EmbedEndpoint,
        done: &'static str,
        cx: &mut Context<Settings>,
    ) {
        if self.locked() {
            return;
        }
        self.embed.saved = None;
        self.endpoint_busy = true;
        let generation = self.endpoint_generation;
        self.endpoint_task = Some(run_backend(
            cx,
            move |backend| backend.set_embed_endpoint(change),
            move |settings: &mut Settings, result, cx| {
                settings.endpoint_busy = false;
                let current = settings.endpoint_generation == generation;
                match result {
                    Ok(view) => {
                        settings.state = Some(ConfigState::new(view.0));
                        if let Some(shell) = settings.shell.upgrade() {
                            shell.update(cx, |shell, cx| shell.refresh_stats(cx));
                        }
                        if !current {
                            return;
                        }
                        settings.embed.errors.clear();
                        settings.embed.saved = Some(done.to_string());
                        settings.embed.replacing = false;
                        settings
                            .draft_resets
                            .insert(embedding::TOKEN.to_string(), String::new());
                    }
                    Err(_) if !current => {}
                    Err(DashboardError::Unprocessable(errors)) => settings.embed.errors = errors,
                    Err(e) => {
                        settings.embed.errors =
                            vec![FieldError::new(embedding::URL, describe_error(&e))]
                    }
                }
            },
        ));
        cx.notify();
    }

    pub fn use_local_ollama(&mut self, cx: &mut Context<Settings>) {
        if self.locked() || self.embed.switching {
            return;
        }
        self.embed.switching = true;
        self.endpoint_busy = true;
        self.endpoint_task = Some(run_backend(
            cx,
            |backend| {
                backend.set_embed_endpoint(EmbedEndpoint {
                    url: Some(None),
                    model: None,
                    token: None,
                })
            },
            |settings: &mut Settings, result, cx| {
                settings.endpoint_busy = false;
                settings.embed.switching = false;
                if let Ok(view) = result {
                    settings.state = Some(ConfigState::new(view.0));
                    if let Some(shell) = settings.shell.upgrade() {
                        shell.update(cx, |shell, cx| shell.refresh_stats(cx));
                    }
                }
            },
        ));
        cx.notify();
    }

    pub fn test_embedding(&mut self, cx: &mut Context<Settings>) {
        if self.embed.testing {
            return;
        }
        self.embed.testing = true;
        self.embed.test = None;
        self.test_task = Some(run_backend(
            cx,
            |backend| backend.embed_test(),
            |settings: &mut Settings, result, _| {
                settings.embed.testing = false;
                settings.embed.test = Some(result);
            },
        ));
        cx.notify();
    }

    pub fn choose_model(&mut self, id: &str, cx: &mut Context<Settings>) {
        let Some(state) = &self.state else {
            return;
        };
        let Some(choice) = state
            .embed_models()
            .into_iter()
            .find(|c| c["id"].as_str() == Some(id))
        else {
            return;
        };
        let number = |key: &str| pending::json_to_toml(&choice[key]);
        self.set(
            "embed.model",
            choice["model"]
                .as_str()
                .map(|m| toml::Value::String(m.to_string())),
            cx,
        );
        self.set("embed.dimensions", number("dimensions"), cx);
        self.set("embed.prefix_scheme", None, cx);
        self.set("hook.threshold", number("hook_threshold"), cx);
        self.set("mcp.threshold", number("mcp_threshold"), cx);
    }

    fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Settings>) {
        let Some(state) = self.state.clone() else {
            return;
        };
        self.sync_embedding();
        for (key, text) in std::mem::take(&mut self.draft_resets) {
            if let Some(input) = self.inputs.get(&key) {
                input.update(cx, |state, cx| state.set_value(text, window, cx));
            }
        }
        let t = *tokens(cx);
        let locked = self.locked();
        for spec in self.input_specs() {
            let value = self.pending.value_of(&state, &spec.path);
            let input = match self.inputs.get(&spec.key) {
                Some(input) => input.clone(),
                None => {
                    let initial = match spec.kind {
                        InputKind::Number { .. } => number_text(value.as_ref()),
                        InputKind::Text { .. } | InputKind::KeepAlive => text_of(value.as_ref()),
                        InputKind::Candidate => String::new(),
                        InputKind::Draft => embedding::draft_initial(&spec.key, &state),
                    };
                    let input = input_state(&spec.placeholder, &initial, &t, window, cx);
                    if spec.masked {
                        input.update(cx, |state, cx| state.set_masked(true, window, cx));
                    }
                    let watched = spec.clone();
                    self.input_events.push(cx.subscribe_in(
                        &input,
                        window,
                        move |settings, state, event, _, cx| match event {
                            InputEvent::Change => {
                                let raw = state.read(cx).value().to_string();
                                settings.input_changed(&watched, raw, cx);
                            }
                            InputEvent::PressEnter { .. }
                                if watched.kind == InputKind::Candidate =>
                            {
                                settings.add_candidate(&watched.path, cx);
                            }
                            _ => {}
                        },
                    ));
                    self.inputs.insert(spec.key.clone(), input.clone());
                    self.synced.insert(spec.key.clone(), value.clone());
                    input
                }
            };
            let text = input.read(cx).value().to_string();
            let wanted = match spec.kind {
                InputKind::Number { .. } => {
                    if self.synced.get(&spec.key) == Some(&value) {
                        None
                    } else {
                        self.synced.insert(spec.key.clone(), value.clone());
                        (!number_shows(&text, value.as_ref())).then(|| number_text(value.as_ref()))
                    }
                }
                InputKind::Text { .. } | InputKind::KeepAlive => {
                    if self.synced.get(&spec.key) == Some(&value) {
                        None
                    } else {
                        self.synced.insert(spec.key.clone(), value.clone());
                        let wanted = text_of(value.as_ref());
                        (wanted != text).then_some(wanted)
                    }
                }
                InputKind::Candidate => {
                    let list = self.lists.entry(spec.path.clone()).or_default();
                    std::mem::take(&mut list.clear).then(String::new)
                }
                InputKind::Draft => None,
            };
            let disabled = locked || spec.disabled;
            input.update(cx, |state, cx| {
                if let Some(wanted) = wanted {
                    state.set_value(wanted, window, cx);
                }
                state.set_disabled(disabled, cx);
            });
            fields::sync_placeholder(&input, &spec.placeholder, window, cx);
        }
    }

    fn input_changed(&mut self, spec: &InputSpec, raw: String, cx: &mut Context<Settings>) {
        match spec.kind {
            InputKind::Number { optional } => {
                let value = parse_number(&raw, optional);
                self.set(&spec.path, value, cx);
            }
            InputKind::Text { optional } => {
                self.synced.remove(&spec.key);
                let value = (!(optional && raw.is_empty())).then(|| toml::Value::String(raw));
                self.set(&spec.path, value, cx);
            }
            InputKind::KeepAlive => {
                self.synced.remove(&spec.key);
                self.set(&spec.path, Some(parse_keep_alive(&raw)), cx);
            }
            InputKind::Draft => cx.notify(),
            InputKind::Candidate => {
                self.lists.entry(spec.path.clone()).or_default().candidate = raw;
                self.revalidate(&spec.path, cx);
                cx.notify();
            }
        }
    }

    fn validates(path: &str) -> bool {
        path == "sources"
    }

    fn revalidate(&mut self, path: &str, cx: &mut Context<Settings>) {
        if !Settings::validates(path) {
            return;
        }
        let Some(state) = &self.state else {
            return;
        };
        let items = string_list(self.pending.value_of(state, path).as_ref());
        let probe_base = self.pending.clone();
        let entry = self.lists.entry(path.to_string()).or_default();
        let trimmed = entry.trimmed().to_string();
        let key = (trimmed.clone(), items.clone());
        if entry.scheduled.as_ref() == Some(&key) {
            return;
        }
        entry.scheduled = Some(key);
        if trimmed.is_empty() {
            entry.task = None;
            return;
        }
        let task_path = path.to_string();
        let mut probe_list: Vec<toml::Value> = items.into_iter().map(toml::Value::String).collect();
        probe_list.push(toml::Value::String(trimmed.clone()));
        let probe = probe_base.with(path, toml::Value::Array(probe_list));
        entry.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHECK_DEBOUNCE).await;
            if !sources::is_absolute(&trimmed) {
                let _ = this.update(cx, |settings, cx| {
                    settings.verdict(&task_path, trimmed, Some(sources::RELATIVE.to_string()));
                    cx.notify();
                });
                return;
            }
            let request = probe.to_check();
            let call = this.update(cx, move |_, cx| {
                run_backend(
                    cx,
                    move |backend| backend.config_check(request),
                    move |settings: &mut Settings, result, _| {
                        let problem = result
                            .ok()
                            .and_then(|c| sources::source_problem(&trimmed, &c.errors));
                        settings.verdict(&task_path, trimmed, problem);
                    },
                )
            });
            if let Ok(call) = call {
                call.await;
            }
        }));
    }

    fn verdict(&mut self, path: &str, candidate: String, problem: Option<String>) {
        self.lists.entry(path.to_string()).or_default().verdict = Some((candidate, problem));
    }

    pub fn add_candidate(&mut self, path: &str, cx: &mut Context<Settings>) {
        let Some(state) = &self.state else {
            return;
        };
        if self.saving {
            return;
        }
        let mut items = string_list(self.pending.value_of(state, path).as_ref());
        let validating = Settings::validates(path);
        let entry = self.lists.entry(path.to_string()).or_default();
        if entry.blocked(&items, validating) {
            return;
        }
        items.push(entry.trimmed().to_string());
        entry.candidate.clear();
        entry.clear = true;
        let value = toml::Value::Array(items.into_iter().map(toml::Value::String).collect());
        self.set(path, Some(value), cx);
    }

    fn start_index(&mut self, reindex: bool, cx: &mut Context<Settings>) {
        self.index
            .update(cx, |status, cx| status.start(reindex, cx));
    }

    fn title_row(&self, t: &Tokens, cx: &App) -> impl IntoElement {
        let h2 = css_line_box(cx, H2_FACE, &[H2_FACE]).baseline;
        let note = css_line_box(cx, XS_FACE, &[XS_FACE]).baseline;
        let line = h2.max(note);
        let label = self.section.label();
        div()
            .flex()
            .items_start()
            .gap(px(12.))
            .child(
                div()
                    .id("settings-title")
                    .test_support()
                    .aria_label(label)
                    .mt(px(line - h2))
                    .child(exact_text(label).semibold().size_xl()),
            )
            .child(
                div()
                    .mt(px(line - note))
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .size_xs()
                    .text_color(t.muted)
                    .child(wrapped_text(TITLE_NOTE)),
            )
    }

    fn conflict_banner(&self, t: &Tokens, cx: &mut Context<Settings>) -> impl IntoElement {
        let text = conflict_text(self.pending.len());
        let locked = self.locked();
        div()
            .id("conflict")
            .test_support()
            .role(Role::Alert)
            .aria_label(SharedString::from(text.clone()))
            .relative()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(12.))
            .size_sm()
            .text_color(t.warn)
            .child(css_box(t.warn_soft, None, 6.))
            .child(div().flex_grow(1.).min_w(px(0.)).child(wrapped_text(text)))
            .child(
                button::warn("conflict-reload", "Reload (discard mine)")
                    .compact()
                    .flex_none()
                    .disabled(locked)
                    .on_click(cx.listener(|s, _, _, cx| s.reload_discarding(cx))),
            )
            .child(
                button::quiet("conflict-keep", "Keep editing")
                    .compact()
                    .flex_none()
                    .disabled(locked)
                    .on_click(cx.listener(|s, _, _, cx| s.keep_editing(cx))),
            )
    }

    fn notice_banner(&self, notice: &Notice, t: &Tokens, cx: &mut Context<Settings>) -> AnyElement {
        let colour = if notice.warn { t.warn } else { t.accent };
        let fill = if notice.warn {
            t.warn_soft
        } else {
            t.accent_soft
        };
        let busy = self.index.read(cx).busy();
        let idle = matches!(self.index.read(cx).state(), IndexState::Idle);
        div()
            .id("settings-notice")
            .test_support()
            .role(Role::Status)
            .aria_label(SharedString::from(notice.text.clone()))
            .relative()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(16.))
            .py(px(10.))
            .size_sm()
            .text_color(colour)
            .child(css_box(fill, None, 6.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .child(wrapped_text(notice.text.clone())),
                    )
                    .child(
                        text_button("settings-notice-dismiss", "dismiss", DISMISS_FACE, colour)
                            .flex_none()
                            .on_click(cx.listener(|s, _, _, cx| s.dismiss_notice(cx))),
                    ),
            )
            .when(notice.reindex, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .text_color(t.warn)
                        .child(
                            div().flex_grow(1.).min_w(px(0.)).child(code_line(
                                &[
                                    Part::Text("The embedding model or dimensions changed, so the index must be rebuilt from scratch ("),
                                    Part::Code("rhebok index --reindex"),
                                    Part::Text("). Search keeps using the old index until it finishes."),
                                ],
                                SM_FACE,
                                REINDEX_CODE_FACE,
                                None,
                                cx,
                            )),
                        )
                        .child(
                            button::warn("settings-full-reindex", "Start full re-index")
                                .flex_none()
                                .disabled(busy)
                                .on_click(cx.listener(|s, _, _, cx| s.start_index(true, cx))),
                        ),
                )
                .when(!idle, |d| {
                    d.child(
                        self.index
                            .read(cx)
                            .element("settings-notice-index-status".into(), t),
                    )
                })
            })
            .into_any_element()
    }

    fn file_errors(
        &self,
        state: &ConfigState,
        t: &Tokens,
        cx: &mut Context<Settings>,
    ) -> AnyElement {
        let errors = state.errors();
        let count = errors.len();
        div()
            .id("file-errors")
            .test_support()
            .relative()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(16.))
            .py(px(12.))
            .border_1()
            .border_color(transparent_black())
            .child(css_box(t.surface, Some(t.warn_line), 8.))
            .child(
                div()
                    .size_sm()
                    .semibold()
                    .text_color(t.warn)
                    .child(wrapped_text(format!(
                        "{count} problem{} already in config.toml",
                        if count == 1 { "" } else { "s" }
                    ))),
            )
            .child(
                div()
                    .size_xs()
                    .text_color(t.muted)
                    .child(wrapped_text(FILE_ERRORS_HINT)),
            )
            .children(errors.into_iter().enumerate().map(|(ix, e)| {
                let path = e.path.clone();
                div()
                    .id(SharedString::from(format!("file-error-{ix}")))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(SharedString::from(e.message.clone()))
                    .flex()
                    .gap(px(8.))
                    .size_sm()
                    .cursor_pointer()
                    .on_click(cx.listener(move |s, _, _, cx| s.jump(&path, cx)))
                    .child(
                        div()
                            .flex_none()
                            .pt(px(2.))
                            .mono()
                            .size_xs()
                            .text_color(t.muted)
                            .child(exact_text(match e.line {
                                Some(line) => format!("line {line}"),
                                None => "—".to_string(),
                            })),
                    )
                    .child(
                        div()
                            .flex_none()
                            .pt(px(2.))
                            .mono()
                            .size_xs()
                            .child(exact_text(if e.path.is_empty() {
                                "syntax".to_string()
                            } else {
                                e.path.clone()
                            })),
                    )
                    .child(
                        div()
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .text_color(t.warn)
                            .child(wrapped_text(e.message.clone())),
                    )
            }))
            .into_any_element()
    }

    fn section_body(&self, ctx: &Ctx, cx: &App) -> Vec<AnyElement> {
        match self.section {
            Section::Sources => sources::sources_section(ctx, &self.lists, &self.index, cx),
            Section::Retrieval => {
                let bench = self
                    .shell
                    .upgrade()
                    .and_then(|shell| shell.read(cx).stats().and_then(|s| s.bench.clone()));
                retrieval::retrieval_section(ctx, bench.as_deref(), cx)
            }
            Section::Embedding => embedding::embedding_section(ctx, &self.embed, cx),
            Section::Memory => memory::memory_section(ctx, cx),
            Section::Documents => documents::documents_section(ctx, cx),
            Section::Updates => updates::updates_section(ctx, cx),
            Section::Advanced => advanced::advanced_section(
                ctx,
                RawView {
                    local: &self.raw,
                    area: self.raw_area.as_ref(),
                    text: self.raw_text().unwrap_or_default(),
                },
                cx,
            ),
        }
    }
}

fn text_of(value: Option<&Json>) -> String {
    match value {
        None | Some(Json::Null) => String::new(),
        Some(Json::String(s)) => s.clone(),
        Some(v) => pending::describe(Some(v)),
    }
}

fn number_shows(text: &str, value: Option<&Json>) -> bool {
    let trimmed = text.trim();
    match value {
        None | Some(Json::Null) => trimmed.is_empty(),
        Some(Json::String(s)) => s == text,
        Some(Json::Number(n)) => !trimmed.is_empty() && trimmed.parse::<f64>().ok() == n.as_f64(),
        Some(_) => false,
    }
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        self.ensure_inputs(window, cx);
        self.ensure_raw(window, cx);
        self.resolve_focus(cx);
        let root = div()
            .id("settings")
            .test_support()
            .aria_label("Settings")
            .size_full();
        let Some(state) = self.state.clone() else {
            if let Some(err) = self.load_err.clone() {
                return root
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .pt(px(32.))
                    .child(banner::error("settings-load-error", err))
                    .child(
                        text_button("settings-retry", "Try again", BUTTON_FACE, t.accent)
                            .underline()
                            .on_click(cx.listener(|s, _, _, cx| s.load(false, cx))),
                    )
                    .into_any_element();
            }
            return root
                .p(px(16.))
                .size_sm()
                .text_color(t.muted)
                .child(baseline::text(LOADING))
                .into_any_element();
        };
        let errors = self.errors();
        let blocking = self.blocking();
        let checking = self.checking();
        let locked = self.locked();
        let ctx = Ctx {
            state: &state,
            pending: &self.pending,
            errors: &errors,
            blocking: &blocking,
            locked,
            view: cx.entity().downgrade(),
            inputs: &self.inputs,
            ring: self.ring.as_deref(),
            bounds: self.bounds.clone(),
            scroll: self.scroll.clone(),
            section: self.section,
        };
        let rows: Vec<NavRow> = SECTIONS
            .iter()
            .map(|&section| NavRow {
                section,
                active: section == self.section,
                problems: errors
                    .iter()
                    .filter(|e| section_of(&e.path) == section)
                    .count(),
                edited: self
                    .pending
                    .paths()
                    .iter()
                    .filter(|p| section_of(p) == section)
                    .count(),
            })
            .collect();
        let view = cx.entity().downgrade();
        let picker = move |section: Section, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |s, cx| s.pick_section(section, cx));
        };
        let body = self.section_body(&ctx, cx);
        let save = (!self.pending.is_empty()).then(|| SaveBar {
            dirty: self.pending.len(),
            checking,
            blocking: blocking.len(),
            first_problem: blocking.first().map(|e| e.path.clone()),
            preview: self.preview,
            saving: self.saving,
            locked,
            can_save: self.can_save(),
            rows: self.pending.diff(state.file(), state.defaults()),
            needs_reindex: self.pending.needs_reindex(),
        });
        let notice = self.notice.clone();
        let pane = div()
            .id("settings-pane")
            .test_support()
            .flex_grow(1.)
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pr(px(4.))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(section_marker(self.bounds.clone(), self.section))
                    .when(self.conflict, |d| d.child(self.conflict_banner(&t, cx)))
                    .when(self.saving, |d| {
                        d.child(
                            div()
                                .id("settings-saving")
                                .test_support()
                                .aria_label(SAVING)
                                .relative()
                                .px(px(16.))
                                .py(px(8.))
                                .size_sm()
                                .text_color(t.muted)
                                .child(css_box(t.sunken, None, 6.))
                                .child(wrapped_text(SAVING)),
                        )
                    })
                    .when_some(notice, |d, notice| {
                        d.child(self.notice_banner(&notice, &t, cx))
                    })
                    .when_some(self.load_err.clone(), |d, err| {
                        d.child(banner::error("settings-load-error", err))
                    })
                    .when(state.unparseable(), |d| {
                        d.child(banner::error("settings-unparseable", UNPARSEABLE))
                    })
                    .when(!state.errors().is_empty(), |d| {
                        d.child(self.file_errors(&state, &t, cx))
                    })
                    .child(self.title_row(&t, cx))
                    .children(body)
                    .child(div().flex_none().h(px(8.))),
            );
        root.flex()
            .gap(px(16.))
            .min_h(px(0.))
            .child(nav(rows, state.path(), state.exists(), picker, cx))
            .child(
                div()
                    .flex_grow(1.)
                    .flex()
                    .flex_col()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .gap(px(12.))
                    .child(pane)
                    .when_some(save, |d, bar| {
                        d.child(deferred(save_bar(bar, cx.entity().downgrade(), cx)))
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod unit {
    use serde_json::json;

    use super::{conflict_text, number_shows};

    #[test]
    fn the_conflict_text_counts_the_unsaved_changes() {
        assert!(conflict_text(1).contains("lose your 1 unsaved change, or"));
        assert!(conflict_text(2).contains("lose your 2 unsaved changes, or"));
    }

    #[test]
    fn a_number_input_resyncs_only_when_its_text_no_longer_says_the_value() {
        assert!(number_shows("30", Some(&json!(30))));
        assert!(number_shows("30.0", Some(&json!(30))));
        assert!(!number_shows("31", Some(&json!(30))));
        assert!(number_shows("", None));
        assert!(number_shows("3x", Some(&json!("3x"))));
    }
}
