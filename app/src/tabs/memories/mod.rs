pub mod editor;
pub mod list;

use chrono::{Local, TimeZone};
use gpui_kit::base::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::dashboard::core::memories::{Memories as MemoryList, MemoryView, ReachedDocument};
use rhebok::dashboard::core::memory_write::{DeleteRequest, SaveOutcome, SaveRequest};

use crate::backend::{describe, run_backend};
use crate::shell::Shell;
use crate::theme::{tokens, Face, Tokens, Type, SANS};
use crate::widgets::text_input::{input_state, textarea_state};
use crate::widgets::{banner, baseline, text_button, tracked};

pub const KINDS: [&str; 3] = ["lesson", "fact", "episode"];
pub const FILTER_KINDS: [&str; 4] = ["all", "lesson", "fact", "episode"];
pub const EVERY_SCOPE: &str = "every scope";
pub const FILTER_PLACEHOLDER: &str = "Filter by text or id…";
pub const PATH_PLACEHOLDER: &str = "/absolute/project/path";
pub const EMPTY_PATH: &str = "Not saved: global is unticked but the project path is empty. Enter an absolute project path, or tick global.";
pub const BAD_CONFIDENCE: &str = "Not saved: confidence must be a whole number from 0 to 100.";
pub const LOADING: &str = "Loading…";
pub const TRY_AGAIN: &str = "Try again";
pub const TEXT_ROWS: usize = 8;

const LABEL_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 12., 16.);
const TRACKING_WIDER: f32 = 0.05;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub id: Option<String>,
    pub kind: String,
    pub text: String,
    pub scope: String,
    pub global: bool,
    pub confidence: String,
}

impl Draft {
    pub fn fresh() -> Draft {
        Draft {
            id: None,
            kind: "lesson".to_string(),
            text: String::new(),
            scope: String::new(),
            global: true,
            confidence: "100".to_string(),
        }
    }

    pub fn from_memory(m: &MemoryView) -> Draft {
        let facts = &m.memory.facts;
        Draft {
            id: Some(m.memory.id.clone()),
            kind: facts.kind.as_str().to_string(),
            text: m.memory.text.clone(),
            scope: facts.project.clone().unwrap_or_default(),
            global: facts.project.is_none(),
            confidence: facts.confidence.to_string(),
        }
    }
}

pub fn line_breaks_as_typed(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn same_draft(a: &Draft, b: &Draft) -> bool {
    a.kind == b.kind
        && line_breaks_as_typed(&a.text) == line_breaks_as_typed(&b.text)
        && a.global == b.global
        && (a.global || a.scope == b.scope)
        && a.confidence == b.confidence
}

pub fn confidence_of(text: &str) -> Option<u8> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = text.trim_start_matches('0');
    if digits.len() > 3 {
        return None;
    }
    let value: u16 = if digits.is_empty() {
        0
    } else {
        digits.parse().ok()?
    };
    (value <= 100).then_some(value as u8)
}

pub fn refusal(d: &Draft) -> Option<&'static str> {
    if !d.global && d.scope.trim().is_empty() {
        return Some(EMPTY_PATH);
    }
    if confidence_of(&d.confidence).is_none() {
        return Some(BAD_CONFIDENCE);
    }
    None
}

pub fn scope_of(m: &MemoryView) -> String {
    match (&m.memory.facts.project, &m.project_name) {
        (None, _) => "global".to_string(),
        (Some(path), Some(name)) if !name.is_empty() => format!("{name} · {path}"),
        (Some(path), _) => path.clone(),
    }
}

pub fn first_line(text: &str) -> String {
    text.split('\n')
        .find(|l| !l.trim().is_empty())
        .unwrap_or(text)
        .trim()
        .to_string()
}

pub fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

pub fn clip(text: &str, max: usize) -> String {
    let line = first_line(text);
    let units: Vec<u16> = line.encode_utf16().collect();
    if units.len() > max {
        format!("{}…", String::from_utf16_lossy(&units[..max]))
    } else {
        line
    }
}

pub fn day(secs: i64) -> String {
    Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|t| t.format("%-m/%-d/%Y").to_string())
        .unwrap_or_default()
}

pub fn moment(secs: i64) -> String {
    Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|t| t.format("%-m/%-d/%Y, %-I:%M:%S\u{202F}%p").to_string())
        .unwrap_or_default()
}

pub fn outcome_id(saved: &SaveOutcome) -> &str {
    match saved {
        SaveOutcome::Saved { id }
        | SaveOutcome::Replaced { id, .. }
        | SaveOutcome::Duplicate { id } => id,
    }
}

pub fn saved_text(saved: &SaveOutcome, edited: Option<&str>) -> String {
    let id = outcome_id(saved);
    let SaveOutcome::Replaced { previous_title, .. } = saved else {
        return format!("Saved as {id}.");
    };
    match edited {
        None => {
            let gone = if previous_title.is_empty() {
                "a memory the server did not name".to_string()
            } else {
                format!("“{previous_title}”")
            };
            format!("Saved as {id}, and it REPLACED an existing memory that said nearly the same thing: {gone}. That earlier memory is gone.")
        }
        Some(edited) if edited == id => format!("Saved your edit to {id}."),
        Some(edited) => format!("Saved your edit as {id} (it was {edited})."),
    }
}

pub fn sort_js(values: &mut [String]) {
    values.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Notice {
    pub text: String,
    pub warn: bool,
    pub open: Option<String>,
    pub stale: Option<String>,
    pub kept: Option<String>,
}

impl Notice {
    pub fn warning(text: impl Into<String>) -> Notice {
        Notice {
            text: text.into(),
            warn: true,
            ..Notice::default()
        }
    }

    pub fn full_text(&self) -> String {
        let mut text = self.text.clone();
        if let Some(kept) = &self.kept {
            text.push(' ');
            text.push_str(kept);
        }
        if let Some(stale) = &self.stale {
            text.push_str(&format!(
                " The list could not be reloaded, so it may be out of date: {stale}"
            ));
        }
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Busy {
    Save(String),
    Delete(String),
}

impl Busy {
    pub fn text(&self) -> String {
        match self {
            Busy::Save(label) => format!("Saving “{label}”. Other memories, New and close are locked until the server answers; embedding can take tens of seconds when Ollama is busy."),
            Busy::Delete(label) => format!("Deleting “{label}”. Other memories, New and close are locked until the server answers."),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leave {
    Pick(String),
    New,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirming {
    pub id: String,
    pub title: String,
}

#[derive(Default)]
struct Inputs {
    filter: Option<Entity<InputState>>,
    text: Option<Entity<TextareaState>>,
    confidence: Option<Entity<InputState>>,
    path: Option<Entity<InputState>>,
    subscriptions: Vec<Subscription>,
}

pub struct Memories {
    shell: WeakEntity<Shell>,
    data: Option<MemoryList>,
    kind: usize,
    project: String,
    needle: String,
    selected_id: Option<String>,
    draft: Option<Draft>,
    baseline: Option<Draft>,
    panel_err: Option<String>,
    notice: Option<Notice>,
    busy: Option<Busy>,
    confirming: Option<Confirming>,
    leaving: Option<Leave>,
    reach: Option<Vec<ReachedDocument>>,
    reach_error: Option<String>,
    reaching: bool,
    reach_for: Option<String>,
    panel_generation: u64,
    writing: bool,
    sync_inputs: bool,
    inputs: Inputs,
    load_task: Option<Task<()>>,
    write_task: Option<Task<()>>,
    reload_task: Option<Task<()>>,
    reach_task: Option<Task<()>>,
    pointer: Option<FocusHandle>,
}

impl Memories {
    pub fn new(shell: WeakEntity<Shell>, cx: &mut Context<Memories>) -> Memories {
        let mut memories = Memories {
            shell,
            data: None,
            kind: 0,
            project: "all".to_string(),
            needle: String::new(),
            selected_id: None,
            draft: None,
            baseline: None,
            panel_err: None,
            notice: None,
            busy: None,
            confirming: None,
            leaving: None,
            reach: None,
            reach_error: None,
            reaching: false,
            reach_for: None,
            panel_generation: 0,
            writing: false,
            sync_inputs: false,
            inputs: Inputs::default(),
            load_task: None,
            write_task: None,
            reload_task: None,
            reach_task: None,
            pointer: None,
        };
        memories.first_load(cx);
        memories
    }

    pub fn draft(&self) -> Option<&Draft> {
        self.draft.as_ref()
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn panel_error(&self) -> Option<&str> {
        self.panel_err.as_deref()
    }

    pub fn writing(&self) -> bool {
        self.writing
    }

    pub fn locked(&self) -> bool {
        self.busy.is_some()
    }

    pub fn busy(&self) -> Option<&Busy> {
        self.busy.as_ref()
    }

    pub fn confirming(&self) -> Option<&Confirming> {
        self.confirming.as_ref()
    }

    pub fn leaving(&self) -> Option<&Leave> {
        self.leaving.as_ref()
    }

    pub fn panel_generation(&self) -> u64 {
        self.panel_generation
    }

    pub fn reach(&self) -> Option<&[ReachedDocument]> {
        self.reach.as_deref()
    }

    pub fn reach_error(&self) -> Option<&str> {
        self.reach_error.as_deref()
    }

    pub fn dirty(&self) -> bool {
        match (&self.draft, &self.baseline) {
            (Some(d), Some(b)) => !same_draft(d, b),
            _ => false,
        }
    }

    pub fn all(&self) -> Vec<&MemoryView> {
        let mut all: Vec<&MemoryView> = self
            .data
            .as_ref()
            .map(|d| d.memories.iter().collect())
            .unwrap_or_default();
        all.sort_by_key(|m| std::cmp::Reverse(m.memory.facts.created));
        all
    }

    pub fn scopes(&self) -> Vec<String> {
        let mut scopes: Vec<String> = self.all().into_iter().map(scope_of).collect();
        sort_js(&mut scopes);
        scopes.dedup();
        scopes
    }

    pub fn scope(&self) -> String {
        if self.project == "all" || self.scopes().contains(&self.project) {
            self.project.clone()
        } else {
            "all".to_string()
        }
    }

    pub fn shown(&self) -> Vec<&MemoryView> {
        let needle = self.needle.trim().to_lowercase();
        let kind = FILTER_KINDS[self.kind];
        let scope = self.scope();
        self.all()
            .into_iter()
            .filter(|m| kind == "all" || m.memory.facts.kind.as_str() == kind)
            .filter(|m| scope == "all" || scope_of(m) == scope)
            .filter(|m| {
                needle.is_empty()
                    || m.memory.text.to_lowercase().contains(&needle)
                    || m.memory.title.to_lowercase().contains(&needle)
                    || m.memory.id.to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub fn shown_ids(&self) -> Vec<String> {
        self.shown()
            .into_iter()
            .map(|m| m.memory.id.clone())
            .collect()
    }

    fn selected(&self) -> Option<&MemoryView> {
        let id = self.selected_id.as_deref()?;
        self.data
            .as_ref()?
            .memories
            .iter()
            .find(|m| m.memory.id == id)
    }

    fn find(&self, id: &str) -> Option<&MemoryView> {
        self.data
            .as_ref()?
            .memories
            .iter()
            .find(|m| m.memory.id == id)
    }

    pub fn first_load(&mut self, cx: &mut Context<Memories>) {
        self.data = None;
        self.load_task = Some(run_backend(
            cx,
            |backend| backend.memories(),
            |memories: &mut Memories, list, _cx| memories.data = Some(list),
        ));
        cx.notify();
    }

    pub fn set_kind_filter(&mut self, kind: usize, cx: &mut Context<Memories>) {
        self.kind = kind.min(FILTER_KINDS.len() - 1);
        cx.notify();
    }

    pub fn set_scope_filter(&mut self, scope: String, cx: &mut Context<Memories>) {
        self.project = scope;
        cx.notify();
    }

    pub fn set_needle(&mut self, needle: String, cx: &mut Context<Memories>) {
        self.needle = needle;
        cx.notify();
    }

    fn open_panel(&mut self, id: Option<String>, next: Option<Draft>) {
        self.panel_generation += 1;
        self.selected_id = id;
        self.draft = next.clone();
        self.baseline = next;
        self.panel_err = None;
        self.confirming = None;
        self.leaving = None;
        self.reach = None;
        self.reach_error = None;
        self.reaching = false;
        self.reach_for = None;
        self.reach_task = None;
        self.sync_inputs = true;
    }

    fn drop_kept(&mut self) {
        if let Some(notice) = &mut self.notice {
            notice.kept = None;
        }
    }

    fn go(&mut self, to: Leave) {
        let id = match to {
            Leave::New => {
                self.drop_kept();
                return self.open_panel(None, Some(Draft::fresh()));
            }
            Leave::Close => {
                self.drop_kept();
                return self.open_panel(None, None);
            }
            Leave::Pick(id) => id,
        };
        let Some(next) = self.find(&id).map(Draft::from_memory) else {
            self.leaving = None;
            self.notice = Some(Notice::warning(format!(
                "{id} is not in the list, so it cannot be opened."
            )));
            return;
        };
        if self.notice.as_ref().and_then(|n| n.open.as_deref()) == Some(id.as_str()) {
            self.notice = None;
        } else {
            self.drop_kept();
        }
        self.open_panel(Some(id), Some(next));
    }

    pub fn request(&mut self, to: Leave, cx: &mut Context<Memories>) {
        if self.writing {
            return;
        }
        if let Leave::Pick(id) = &to {
            if self.selected_id.as_deref() == Some(id.as_str()) {
                return;
            }
        }
        if self.dirty() {
            self.leaving = Some(to);
        } else {
            self.go(to);
        }
        cx.notify();
    }

    pub fn discard(&mut self, cx: &mut Context<Memories>) {
        if self.writing {
            return;
        }
        if let Some(to) = self.leaving.clone() {
            self.go(to);
        }
        cx.notify();
    }

    pub fn keep_editing(&mut self, cx: &mut Context<Memories>) {
        self.leaving = None;
        cx.notify();
    }

    fn edit(&mut self, patch: impl FnOnce(&mut Draft), cx: &mut Context<Memories>) {
        self.panel_generation += 1;
        self.panel_err = None;
        if let Some(draft) = &mut self.draft {
            patch(draft);
        }
        cx.notify();
    }

    pub fn set_text(&mut self, text: String, cx: &mut Context<Memories>) {
        self.sync_inputs = true;
        self.edit(|d| d.text = text, cx);
    }

    pub fn set_kind(&mut self, kind: String, cx: &mut Context<Memories>) {
        self.edit(|d| d.kind = kind, cx);
    }

    pub fn set_confidence(&mut self, confidence: String, cx: &mut Context<Memories>) {
        self.sync_inputs = true;
        self.edit(|d| d.confidence = confidence, cx);
    }

    pub fn set_global(&mut self, global: bool, cx: &mut Context<Memories>) {
        self.edit(|d| d.global = global, cx);
    }

    pub fn set_path(&mut self, scope: String, cx: &mut Context<Memories>) {
        self.sync_inputs = true;
        self.edit(|d| d.scope = scope, cx);
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Memories>) {
        self.notice = None;
        cx.notify();
    }

    pub fn press_delete(&mut self, cx: &mut Context<Memories>) {
        if self.locked() {
            return;
        }
        if let Some(m) = self.selected() {
            self.confirming = Some(Confirming {
                id: m.memory.id.clone(),
                title: m.memory.title.clone(),
            });
        }
        cx.notify();
    }

    pub fn keep_it(&mut self, cx: &mut Context<Memories>) {
        if self.locked() {
            return;
        }
        self.confirming = None;
        cx.notify();
    }

    pub fn confirm_delete(&mut self, cx: &mut Context<Memories>) {
        if let Some(target) = self.confirming.clone() {
            self.remove(target, cx);
        }
    }

    fn reload(&mut self, cx: &mut Context<Memories>) {
        self.reload_task = Some(run_backend(
            cx,
            |backend| backend.memories(),
            |memories: &mut Memories, list, _cx| {
                let stale = list.unavailable.clone();
                if stale.is_none() {
                    memories.data = Some(list);
                }
                if let Some(notice) = &mut memories.notice {
                    notice.stale = stale;
                }
            },
        ));
    }

    pub fn reload_list(&mut self, cx: &mut Context<Memories>) {
        self.reload(cx);
        cx.notify();
    }

    fn memory_written(&self, cx: &mut Context<Memories>) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |shell, cx| shell.memory_written(cx));
        }
    }

    pub fn save(&mut self, cx: &mut Context<Memories>) {
        let Some(draft) = self.draft.clone() else {
            return;
        };
        if self.writing {
            return;
        }
        if let Some(problem) = refusal(&draft) {
            self.notice = None;
            self.panel_err = Some(problem.to_string());
            cx.notify();
            return;
        }
        let sent = draft;
        let unchanged_text = self
            .baseline
            .as_ref()
            .is_some_and(|b| line_breaks_as_typed(&sent.text) == line_breaks_as_typed(&b.text));
        let text = match (&self.baseline, unchanged_text) {
            (Some(b), true) => b.text.clone(),
            _ => sent.text.clone(),
        };
        let token = self.panel_generation;
        self.writing = true;
        self.busy = Some(Busy::Save(clip(&sent.text, 60)));
        self.panel_err = None;
        self.notice = None;
        self.confirming = None;
        self.leaving = None;
        let request = SaveRequest {
            id: sent.id.clone(),
            kind: sent.kind.clone(),
            text,
            scope: if sent.global {
                "global".to_string()
            } else {
                sent.scope.clone()
            },
            confidence: confidence_of(&sent.confidence),
        };
        self.write_task = Some(run_backend(
            cx,
            move |backend| backend.memory_save(request),
            move |memories: &mut Memories, result, cx| match result {
                Err(e) => {
                    if memories.panel_generation == token {
                        memories.panel_err = Some(e.to_string());
                    } else {
                        memories.notice = Some(Notice::warning(format!(
                            "“{}” was not saved: {e}",
                            clip(&sent.text, 60)
                        )));
                    }
                    memories.finish_write();
                }
                Ok(saved) => {
                    if !matches!(saved, SaveOutcome::Duplicate { .. }) {
                        memories.memory_written(cx);
                    }
                    memories.reload_task = Some(run_backend(
                        cx,
                        |backend| backend.memories(),
                        move |memories: &mut Memories, list, _cx| {
                            memories.saved(saved, sent, token, list);
                        },
                    ));
                }
            },
        ));
        cx.notify();
    }

    fn finish_write(&mut self) {
        self.writing = false;
        self.busy = None;
    }

    fn saved(&mut self, saved: SaveOutcome, sent: Draft, token: u64, list: MemoryList) {
        let still_here = self.panel_generation == token;
        let (fresh, stale) = match list.unavailable.clone() {
            Some(why) => (false, Some(why)),
            None => {
                self.data = Some(list);
                (true, None)
            }
        };
        let id = outcome_id(&saved).to_string();
        if let SaveOutcome::Duplicate { .. } = saved {
            self.notice = Some(Notice {
                text: format!("Nothing was written: a very similar memory already exists as {id}."),
                kept: still_here.then(|| "What you typed is still in the panel.".to_string()),
                warn: true,
                open: Some(id),
                stale,
            });
            self.finish_write();
            return;
        }
        let landed = if fresh {
            self.find(&id).map(Draft::from_memory)
        } else {
            None
        };
        let replaced_another = matches!(saved, SaveOutcome::Replaced { .. }) && sent.id.is_none();
        let text = saved_text(&saved, sent.id.as_deref());
        if fresh && landed.is_none() {
            self.notice = Some(Notice::warning(format!(
                "{text} But no memory with id {id} came back when the list reloaded."
            )));
        } else {
            self.notice = Some(Notice {
                text,
                warn: replaced_another,
                stale,
                ..Notice::default()
            });
        }
        match landed {
            Some(draft) if still_here => self.open_panel(Some(id), Some(draft)),
            None if still_here && !fresh => {
                let after = Draft {
                    id: Some(id.clone()),
                    ..sent
                };
                self.open_panel(Some(id), Some(after));
            }
            _ => {}
        }
        self.finish_write();
    }

    fn remove(&mut self, target: Confirming, cx: &mut Context<Memories>) {
        if self.writing {
            return;
        }
        if self.selected_id.as_deref() != Some(target.id.as_str()) {
            self.confirming = None;
            self.notice = Some(Notice::warning(format!(
                "Nothing was deleted: the panel no longer shows “{}”, the memory you confirmed.",
                target.title
            )));
            cx.notify();
            return;
        }
        let token = self.panel_generation;
        self.writing = true;
        self.busy = Some(Busy::Delete(target.title.clone()));
        self.panel_err = None;
        self.notice = None;
        self.leaving = None;
        let id = target.id.clone();
        self.write_task = Some(run_backend(
            cx,
            move |backend| backend.memory_delete(DeleteRequest { id }),
            move |memories: &mut Memories, result, cx| match result {
                Err(e) => {
                    if memories.panel_generation == token {
                        memories.panel_err = Some(e.to_string());
                    } else {
                        memories.notice = Some(Notice::warning(format!(
                            "“{}” was not deleted: {e}",
                            target.title
                        )));
                    }
                    memories.finish_write();
                }
                Ok(gone) => {
                    memories.memory_written(cx);
                    memories.reload_task = Some(run_backend(
                        cx,
                        |backend| backend.memories(),
                        move |memories: &mut Memories, list, _cx| {
                            let stale = list.unavailable.clone();
                            if stale.is_none() {
                                memories.data = Some(list);
                            }
                            if memories.panel_generation == token {
                                memories.open_panel(None, None);
                            }
                            memories.notice = Some(Notice {
                                text: format!("Deleted “{}”.", gone.title),
                                warn: false,
                                stale,
                                ..Notice::default()
                            });
                            memories.finish_write();
                        },
                    ));
                }
            },
        ));
        cx.notify();
    }

    pub fn load_reach(&mut self, cx: &mut Context<Memories>) {
        if self.writing {
            return;
        }
        let Some(id) = self.selected().map(|m| m.memory.id.clone()) else {
            return;
        };
        self.reach_for = Some(id.clone());
        self.reaching = true;
        self.reach = None;
        self.reach_error = None;
        let asked = id.clone();
        self.reach_task = Some(run_backend(
            cx,
            move |backend| backend.memory_reach(asked),
            move |memories: &mut Memories, result, _cx| {
                if memories.reach_for.as_deref() != Some(id.as_str()) {
                    return;
                }
                match result {
                    Ok(r) => match r.error {
                        Some(error) => memories.reach_error = Some(error),
                        None => memories.reach = Some(r.reach.unwrap_or_default()),
                    },
                    Err(e) => memories.reach_error = Some(describe(&e)),
                }
                memories.reaching = false;
            },
        ));
        cx.notify();
    }

    fn take_focus(&mut self, window: &mut Window, cx: &mut Context<Memories>) {
        let pointer = self
            .pointer
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        window.focus(&pointer, cx);
    }

    fn ensure_inputs(&mut self, window: &mut Window, cx: &mut Context<Memories>) {
        let t = *tokens(cx);
        if self.inputs.filter.is_none() {
            let filter = input_state(FILTER_PLACEHOLDER, &self.needle, &t, window, cx);
            let text = textarea_state("", TEXT_ROWS, &t, window, cx);
            let confidence = input_state("", "", &t, window, cx);
            let path = input_state(PATH_PLACEHOLDER, "", &t, window, cx);
            self.inputs.subscriptions = vec![
                cx.subscribe_in(&filter, window, |m, state, event, _, cx| {
                    if let InputEvent::Change = event {
                        let value = state.read(cx).value().to_string();
                        m.set_needle(value, cx);
                    }
                }),
                cx.subscribe_in(&text, window, |m, state, event, _, cx| {
                    if let InputEvent::Change = event {
                        let value = state.read(cx).value().to_string();
                        m.edit(|d| d.text = value, cx);
                    }
                }),
                cx.subscribe_in(&confidence, window, |m, state, event, _, cx| {
                    if let InputEvent::Change = event {
                        let value = state.read(cx).value().to_string();
                        m.edit(|d| d.confidence = value, cx);
                    }
                }),
                cx.subscribe_in(&path, window, |m, state, event, _, cx| {
                    if let InputEvent::Change = event {
                        let value = state.read(cx).value().to_string();
                        m.edit(|d| d.scope = value, cx);
                    }
                }),
            ];
            self.inputs.filter = Some(filter);
            self.inputs.text = Some(text);
            self.inputs.confidence = Some(confidence);
            self.inputs.path = Some(path);
        }
        let locked = self.locked();
        let global = self.draft.as_ref().is_none_or(|d| d.global);
        if let (Some(text), Some(confidence), Some(path)) = (
            self.inputs.text.clone(),
            self.inputs.confidence.clone(),
            self.inputs.path.clone(),
        ) {
            if std::mem::take(&mut self.sync_inputs) {
                if let Some(draft) = self.draft.clone() {
                    text.update(cx, |s, cx| s.set_value(draft.text, window, cx));
                    confidence.update(cx, |s, cx| s.set_value(draft.confidence, window, cx));
                    path.update(cx, |s, cx| s.set_value(draft.scope, window, cx));
                }
            }
            text.update(cx, |s, cx| s.set_readonly(locked, cx));
            confidence.update(cx, |s, cx| s.set_disabled(locked, cx));
            path.update(cx, |s, cx| s.set_disabled(global || locked, cx));
        }
    }
}

pub fn kind_shade(t: &Tokens, kind: &str) -> Hsla {
    match kind {
        "lesson" => t.graph.lesson,
        "fact" => t.graph.fact,
        "episode" => t.graph.episode,
        _ => t.graph.memory,
    }
}

pub fn kind_chip(t: &Tokens, kind: &str) -> Div {
    div().flex_none().child(
        tracked(
            kind.to_uppercase(),
            SANS,
            FontWeight::SEMIBOLD,
            px(10.),
            TRACKING_WIDER,
        )
        .line_height(px(15.))
        .text_color(t.ink)
        .framed(kind_shade(t, kind), 4., 6., 2.),
    )
}

pub fn small_caps(text: &str, t: &Tokens) -> impl IntoElement {
    tracked(
        text.to_uppercase(),
        SANS,
        FontWeight::SEMIBOLD,
        px(11.),
        TRACKING_WIDER,
    )
    .line_height(px(16.5))
    .text_color(t.muted)
}

impl Memories {
    fn unavailable_view(&self, reason: String, t: &Tokens, cx: &mut Context<Memories>) -> Div {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .pt(px(32.))
            .child(banner::error("memories-unavailable", reason))
            .child(
                text_button("memories-retry", TRY_AGAIN, LABEL_FACE, t.accent)
                    .underline()
                    .on_click(cx.listener(|m, _, _, cx| m.first_load(cx))),
            )
    }
}

impl Render for Memories {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let pointer = self
            .pointer
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let root = div()
            .id("memories")
            .track_focus(&pointer)
            .test_support()
            .aria_label("Memories")
            .size_full();
        let unavailable = self.data.as_ref().and_then(|d| d.unavailable.clone());
        if let Some(reason) = unavailable {
            return root.child(self.unavailable_view(reason, &t, cx));
        }
        if self.data.is_none() {
            return root.child(
                div()
                    .id("memories-loading")
                    .test_support()
                    .aria_label(LOADING)
                    .size_sm()
                    .text_color(t.muted)
                    .p(px(16.))
                    .child(baseline::text(LOADING)),
            );
        }
        self.ensure_inputs(window, cx);
        let toolbar = self.toolbar(&t, cx);
        let busy = self.busy.as_ref().map(|busy| {
            let text = busy.text();
            div()
                .id("memories-busy")
                .test_support()
                .aria_label(SharedString::from(text.clone()))
                .rounded(px(6.))
                .px(px(16.))
                .py(px(8.))
                .size_sm()
                .bg(t.sunken)
                .text_color(t.muted)
                .child(crate::widgets::wrap::wrapped_text(text))
        });
        let notice = self.notice_view(&t, cx);
        let list = self.list(&t, cx);
        let editor = self.draft.is_some().then(|| self.editor(&t, window, cx));
        root.child(
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(14.))
                .min_h(px(0.))
                .child(toolbar)
                .when_some(busy, |d, busy| d.child(busy))
                .when_some(notice, |d, notice| d.child(notice))
                .child(
                    div()
                        .flex()
                        .gap(px(16.))
                        .flex_grow(1.)
                        .min_h(px(0.))
                        .child(list)
                        .when_some(editor, |d, editor| d.child(editor)),
                ),
        )
    }
}

#[cfg(test)]
mod tests;
