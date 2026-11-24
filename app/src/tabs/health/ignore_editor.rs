use std::time::Duration;

use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use rhebok::dashboard::core::config::ConfigSave;
use rhebok::dashboard::core::ignore::IgnorePreview;
use rhebok::dashboard::core::DashboardError;
use rhebok::skip::Skip;

use crate::backend::{describe, run_backend, Backend};
use crate::theme::{tokens, Type};
use crate::widgets::text_input::input_state;
use crate::widgets::{baseline, button, exact_text, text_input};

pub const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(250);
pub const CONFLICT_TWICE: &str = "config.toml changed while saving. Try again.";
const HINT: &str = "A name skips any file or folder with that name. A path skips it and everything under it. A glob such as *.tmp or drafts/**/*.md matches paths.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub pattern: String,
}

fn base_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |at| &path[at + 1..])
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) if at >= 1 => &path[..at],
        _ => path
            .char_indices()
            .nth(1)
            .map_or(path, |(at, _)| &path[..at]),
    }
}

fn home_of(path: &str) -> Option<&str> {
    let rest = path
        .strip_prefix("/Users/")
        .or_else(|| path.strip_prefix("/home/"))?;
    let user = rest.split('/').next().unwrap_or("");
    if user.is_empty() {
        return None;
    }
    Some(&path[..path.len() - rest.len() + user.len()])
}

pub fn tilde(path: &str) -> String {
    match home_of(path) {
        Some(home) => format!("~{}", &path[home.len()..]),
        None => path.to_string(),
    }
}

fn common_folder(paths: &[&str]) -> String {
    let split: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| parent_of(p).split('/').collect())
        .collect();
    let first = split.first().cloned().unwrap_or_default();
    let mut n = 0;
    while n < first.len() && split.iter().all(|s| s.get(n) == Some(&first[n])) {
        n += 1;
    }
    first[..n].join("/")
}

pub fn short_path(path: &str) -> (Option<String>, String) {
    let rel = tilde(path);
    let session = rel.strip_prefix("~/.claude/projects/").and_then(|rest| {
        let (dir, shown) = rest.split_once('/')?;
        (!dir.is_empty()).then_some((dir, shown))
    });
    match (session, home_of(path)) {
        (Some((dir, shown)), Some(home)) => {
            let encoded = home.replace('/', "-");
            let project = if dir.starts_with(&encoded) {
                dir.get(encoded.len() + 1..).unwrap_or("")
            } else {
                dir
            };
            let project = if project.is_empty() { "home" } else { project };
            (Some(project.to_string()), shown.to_string())
        }
        _ => (None, rel),
    }
}

pub fn suggestions_for(items: &[Skip]) -> Vec<Suggestion> {
    let mut out = Vec::new();
    if let [only] = items {
        out.push(Suggestion {
            label: "this file".to_string(),
            pattern: tilde(&only.path),
        });
        out.push(Suggestion {
            label: "its folder".to_string(),
            pattern: tilde(parent_of(&only.path)),
        });
    }
    let mut names: Vec<(&str, usize)> = Vec::new();
    for item in items {
        let name = base_name(&item.path);
        match names.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => names.push((name, 1)),
        }
    }
    let top = names
        .iter()
        .fold(None::<(&str, usize)>, |best, &(name, count)| match best {
            Some((_, most)) if most >= count => best,
            _ => Some((name, count)),
        });
    if let Some((name, count)) = top {
        if !name.is_empty() && (count > 1 || items.len() == 1) {
            let workflow = items
                .iter()
                .any(|s| s.path.contains("/subagents/workflows/") && base_name(&s.path) == name);
            if workflow {
                out.push(Suggestion {
                    label: format!("every workflow {name}"),
                    pattern: format!("subagents/workflows/*/{name}"),
                });
            }
            out.push(Suggestion {
                label: format!("every file named {name}"),
                pattern: name.to_string(),
            });
        }
    }
    if items.len() > 1 {
        let paths: Vec<&str> = items.iter().map(|s| s.path.as_str()).collect();
        let folder = common_folder(&paths);
        if !folder.is_empty()
            && home_of(&folder) != Some(folder.as_str())
            && folder.split('/').count() > 3
        {
            out.push(Suggestion {
                label: "their shared folder".to_string(),
                pattern: tilde(&folder),
            });
        }
    }
    out
}

fn current_ignore(view: &serde_json::Value) -> Vec<String> {
    view["effective"]["ignore"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn add_to_ignore(backend: &dyn Backend, entry: &str) -> Result<String, String> {
    for _ in 0..2 {
        let view = backend.config_view().map_err(|e| describe(&e))?;
        let mut ignore = current_ignore(&view.0);
        if ignore.iter().any(|e| e == entry) {
            return Ok(entry.to_string());
        }
        ignore.push(entry.to_string());
        let mut set = serde_json::Map::new();
        set.insert("ignore".to_string(), serde_json::json!(ignore));
        let save = ConfigSave {
            etag: view.0["etag"].as_str().unwrap_or_default().to_string(),
            set,
            unset: Vec::new(),
        };
        match backend.config_save(save) {
            Ok(_) => return Ok(entry.to_string()),
            Err(DashboardError::ConfigChanged { .. }) => continue,
            Err(DashboardError::Unprocessable(errors)) if !errors.is_empty() => {
                return Err(errors
                    .iter()
                    .map(|e| e.message.as_str())
                    .collect::<Vec<_>>()
                    .join(" "))
            }
            Err(e) => return Err(describe(&e)),
        }
    }
    Err(CONFLICT_TWICE.to_string())
}

pub fn preview_line(preview: Option<&IgnorePreview>) -> String {
    match preview {
        Some(IgnorePreview {
            error: Some(error), ..
        }) => error.clone(),
        Some(p) => format!(
            "Hides {} of the {} skipped files listed here. Notes and sessions it matches are no longer indexed either.",
            p.matches, p.of
        ),
        None => HINT.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEvent {
    Cancelled,
    Added(String),
}

pub struct IgnoreEditor {
    suggestions: Vec<Suggestion>,
    input: Entity<InputState>,
    pattern: String,
    preview: Option<IgnorePreview>,
    preview_task: Option<Task<()>>,
    saving: bool,
    error: Option<String>,
    save_task: Option<Task<()>>,
    _input_events: Subscription,
}

impl EventEmitter<EditorEvent> for IgnoreEditor {}

impl IgnoreEditor {
    pub fn new(items: &[Skip], window: &mut Window, cx: &mut Context<Self>) -> IgnoreEditor {
        let suggestions = suggestions_for(items);
        let pattern = suggestions
            .first()
            .map(|s| s.pattern.clone())
            .unwrap_or_default();
        let t = *tokens(cx);
        let input = input_state("", &pattern, &t, window, cx);
        let subscription = cx.subscribe_in(&input, window, |editor, state, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = state.read(cx).value().to_string();
                editor.set_pattern(value, cx);
            }
        });
        let mut editor = IgnoreEditor {
            suggestions,
            input,
            pattern: String::new(),
            preview: None,
            preview_task: None,
            saving: false,
            error: None,
            save_task: None,
            _input_events: subscription,
        };
        editor.set_pattern(pattern, cx);
        editor
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    fn set_pattern(&mut self, pattern: String, cx: &mut Context<Self>) {
        if pattern == self.pattern {
            return;
        }
        self.pattern = pattern;
        let entry = self.pattern.trim().to_string();
        if entry.is_empty() {
            self.preview = None;
            self.preview_task = None;
            cx.notify();
            return;
        }
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            let call = this.update(cx, |_, cx| {
                run_backend(
                    cx,
                    move |backend| backend.ignore_preview(entry),
                    |editor: &mut IgnoreEditor, result, _| editor.preview = result.ok(),
                )
            });
            if let Ok(call) = call {
                call.await;
            }
        }));
        cx.notify();
    }

    fn choose(&mut self, pattern: String, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |state, cx| state.set_value(pattern.clone(), window, cx));
        self.set_pattern(pattern, cx);
    }

    fn blocked(&self) -> bool {
        self.pattern.trim().is_empty()
            || self.preview.as_ref().is_some_and(|p| p.error.is_some())
            || self.saving
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        if self.blocked() {
            return;
        }
        let entry = self.pattern.trim().to_string();
        self.saving = true;
        self.error = None;
        self.save_task = Some(run_backend(
            cx,
            move |backend| add_to_ignore(backend, &entry),
            |editor: &mut IgnoreEditor, outcome, cx| {
                editor.saving = false;
                match outcome {
                    Ok(pattern) => cx.emit(EditorEvent::Added(pattern)),
                    Err(message) => editor.error = Some(message),
                }
            },
        ));
        cx.notify();
    }
}

impl Render for IgnoreEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let preview = preview_line(self.preview.as_ref());
        let preview_failed = self.preview.as_ref().is_some_and(|p| p.error.is_some());
        let blocked = self.blocked();
        div()
            .id("ignore-editor")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(12.))
            .bg(t.sunken.opacity(0.6))
            .rounded(px(6.))
            .size_sm()
            .when(!self.suggestions.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.))
                        .child(exact_text("Ignore").size_xs().text_color(t.muted))
                        .children(self.suggestions.iter().enumerate().map(|(ix, s)| {
                            let on = s.pattern == self.pattern;
                            let pattern = s.pattern.clone();
                            div()
                                .id(SharedString::from(format!("ignore-suggestion-{ix}")))
                                .test_support()
                                .role(Role::Button)
                                .aria_label(s.label.clone())
                                .cursor_pointer()
                                .flex_none()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded(px(4.))
                                .border_1()
                                .bg(t.surface)
                                .size_xs()
                                .map(|d| {
                                    if on {
                                        d.border_color(t.accent).text_color(t.accent).semibold()
                                    } else {
                                        d.border_color(t.line).text_color(t.ink).regular()
                                    }
                                })
                                .on_click(cx.listener(move |editor, _, window, cx| {
                                    editor.choose(pattern.clone(), window, cx)
                                }))
                                .child(exact_text(s.label.clone()))
                        })),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        text_input("ignore-entry", "Ignore entry", &self.input)
                            .flex_grow(1.)
                            .flex_basis(px(320.))
                            .min_w(px(0.))
                            .px(px(10.))
                            .py(px(6.))
                            .mono()
                            .size_xs(),
                    )
                    .child(
                        button::primary("ignore-add", "Add to ignore list")
                            .small()
                            .disabled(blocked)
                            .busy_label(self.saving.then(|| "Adding…".into()))
                            .on_click(cx.listener(|editor, _, _, cx| editor.add(cx))),
                    )
                    .child(
                        button::secondary("ignore-cancel", "Cancel")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(EditorEvent::Cancelled))),
                    ),
            )
            .child(
                div()
                    .id("ignore-preview")
                    .test_support()
                    .aria_label(preview.clone())
                    .size_xs()
                    .text_color(if preview_failed { t.warn } else { t.muted })
                    .child(baseline::text(preview)),
            )
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .id("ignore-error")
                        .test_support()
                        .aria_label(error.clone())
                        .size_xs()
                        .text_color(t.warn)
                        .child(baseline::text(error)),
                )
            })
    }
}
