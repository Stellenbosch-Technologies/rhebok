use std::collections::{BTreeSet, HashSet};

use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use rhebok::skip::{Skip, SkipKind};

use super::ignore_editor::{short_path, EditorEvent, IgnoreEditor};
use crate::shell::Shell;
use crate::theme::{tokens, Face, Tokens, Type, SANS, SM_FACE, XS_MONO_FACE};
use crate::widgets::css_box::{css_box, css_rule_top};
use crate::widgets::tail::tail_text;
use crate::widgets::text_input::input_state;
use crate::widgets::{
    baseline, button, chevron, dotted_link, exact_text, on_web_baseline, panel, text_input, words,
};

pub const SHOWN_PER_GROUP: usize = 50;
pub const ROWS_MAX_HEIGHT: f32 = 384.;
const CHIP_LINE: f32 = 11. * 20. / 14.;
const SM_MONO_LINE: [Face; 2] = [SM_FACE, XS_MONO_FACE];

pub const ORDER: [SkipKind; 5] = [
    SkipKind::Unreadable,
    SkipKind::Failed,
    SkipKind::Scanned,
    SkipKind::Settling,
    SkipKind::Empty,
];

pub fn kind_key(kind: SkipKind) -> &'static str {
    match kind {
        SkipKind::Empty => "empty",
        SkipKind::Settling => "settling",
        SkipKind::Unreadable => "unreadable",
        SkipKind::Scanned => "scanned",
        SkipKind::Failed => "failed",
    }
}

pub fn kind_text(kind: SkipKind) -> (&'static str, &'static str) {
    match kind {
        SkipKind::Empty => ("Empty sessions", "These session files hold no messages, so there is nothing to index. Ignoring them loses nothing."),
        SkipKind::Settling => ("Still being written", "A session is still adding to these. The first index after ten quiet minutes reads them."),
        SkipKind::Unreadable => ("Could not be read", "The operating system refused to open these files. The error code says why."),
        SkipKind::Scanned => ("Scanned PDFs", "These PDFs have no text layer. Turn on OCR in Settings to read them."),
        SkipKind::Failed => ("Could not be indexed", "rhebok opened these files but could not turn them into documents. The detail names the problem."),
    }
}

pub fn row_key(item: &Skip) -> String {
    format!("{}{}", item.path, item.detail)
}

pub fn groups(skipped: &[Skip], filter: &str) -> Vec<(SkipKind, Vec<Skip>)> {
    let q = filter.trim().to_lowercase();
    ORDER
        .iter()
        .filter_map(|kind| {
            let items: Vec<Skip> = skipped
                .iter()
                .filter(|s| s.kind == *kind)
                .filter(|s| {
                    let io = s.io.as_ref().map_or("", |io| io.kind.as_str());
                    q.is_empty()
                        || format!("{} {} {}", s.path, s.detail, io)
                            .to_lowercase()
                            .contains(&q)
                })
                .cloned()
                .collect();
            (!items.is_empty()).then_some((*kind, items))
        })
        .collect()
}

pub fn count_line(total: usize) -> String {
    if total == 0 {
        "none".to_string()
    } else {
        format!("{total} in the last index run")
    }
}

pub fn ignore_label(count: usize) -> String {
    if count == 1 {
        "Ignore…".to_string()
    } else {
        format!("Ignore all {count}…")
    }
}

pub fn empty_line(hidden: usize) -> String {
    format!(
        "The last index run read every file it found{}.",
        if hidden > 0 {
            " that is not on your ignore list"
        } else {
            ""
        }
    )
}

struct Editing {
    key: String,
    editor: Entity<IgnoreEditor>,
    _events: Subscription,
}

pub struct SkippedPanel {
    shell: WeakEntity<Shell>,
    skipped: Vec<Skip>,
    hidden: usize,
    filter: Option<Entity<InputState>>,
    filter_text: String,
    open: BTreeSet<SkipKind>,
    whole: BTreeSet<SkipKind>,
    rows_open: HashSet<String>,
    copied: HashSet<String>,
    editing: Option<Editing>,
    added: Option<String>,
    _filter_events: Option<Subscription>,
}

impl SkippedPanel {
    pub fn new(shell: WeakEntity<Shell>) -> SkippedPanel {
        SkippedPanel {
            shell,
            skipped: Vec::new(),
            hidden: 0,
            filter: None,
            filter_text: String::new(),
            open: BTreeSet::new(),
            whole: BTreeSet::new(),
            rows_open: HashSet::new(),
            copied: HashSet::new(),
            editing: None,
            added: None,
            _filter_events: None,
        }
    }

    pub fn set_skipped(&mut self, skipped: Vec<Skip>, hidden: usize, cx: &mut Context<Self>) {
        if skipped != self.skipped || hidden != self.hidden {
            self.skipped = skipped;
            self.hidden = hidden;
            cx.notify();
        }
    }

    fn open_setting(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |shell, cx| shell.open_setting(key, cx));
        }
    }

    fn toggle_group(&mut self, kind: SkipKind, cx: &mut Context<Self>) {
        if !self.open.remove(&kind) {
            self.open.insert(kind);
        }
        cx.notify();
    }

    fn toggle_row(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.rows_open.remove(&key) {
            self.rows_open.insert(key);
        }
        cx.notify();
    }

    fn edit(&mut self, key: String, items: Vec<Skip>, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.as_ref().is_some_and(|e| e.key == key) {
            return;
        }
        let editor = cx.new(|cx| IgnoreEditor::new(&items, window, cx));
        let events = cx.subscribe(&editor, |panel, _, event: &EditorEvent, cx| {
            panel.editing = None;
            if let EditorEvent::Added(pattern) = event {
                panel.added = Some(pattern.clone());
                if let Some(shell) = panel.shell.upgrade() {
                    shell.update(cx, |shell, cx| shell.refresh_stats(cx));
                }
            }
            cx.notify();
        });
        self.editing = Some(Editing {
            key,
            editor,
            _events: events,
        });
        cx.notify();
    }

    fn editor_for(&self, key: &str) -> Option<Entity<IgnoreEditor>> {
        self.editing
            .as_ref()
            .filter(|e| e.key == key)
            .map(|e| e.editor.clone())
    }

    fn ensure_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(filter) = &self.filter {
            return filter.clone();
        }
        let t = *tokens(cx);
        let filter = input_state("Filter by path, reason or error", "", &t, window, cx);
        self._filter_events =
            Some(
                cx.subscribe_in(&filter, window, |panel, state, event, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        panel.filter_text = state.read(cx).value().to_string();
                        cx.notify();
                    }
                }),
            );
        self.filter = Some(filter.clone());
        filter
    }

    fn aside(&mut self, t: &Tokens, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let count = count_line(self.skipped.len());
        let mut aside = div()
            .flex()
            .flex_grow(1.)
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .id("skipped-count")
                    .test_support()
                    .aria_label(count.clone())
                    .size_sm()
                    .tabular()
                    .text_color(t.muted)
                    .child(exact_text(count)),
            );
        if self.hidden > 0 {
            aside = aside.child(
                dotted_link(
                    "skipped-hidden",
                    format!("{} more hidden by your ignore list", self.hidden),
                    SM_FACE,
                )
                .on_click(cx.listener(|panel, _, _, cx| panel.open_setting("ignore", cx))),
            );
        }
        aside = aside.child(div().flex_grow(1.));
        if !self.skipped.is_empty() {
            let filter = self.ensure_filter(window, cx);
            aside = aside.child(
                text_input("skipped-filter", "Filter skipped files", &filter)
                    .w(px(256.))
                    .max_w_full()
                    .flex_none()
                    .px(px(10.))
                    .py(px(4.))
                    .background(t.ground)
                    .regular()
                    .size_sm(),
            );
        }
        aside.into_any_element()
    }

    fn added_banner(&self, added: &str, t: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let text = format!(
            "Added {added} to the ignore list. Matching files are hidden here now, and the next index does not read them."
        );
        div()
            .id("ignore-added")
            .test_support()
            .aria_label(text)
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(12.))
            .bg(t.accent_soft)
            .rounded(px(6.))
            .px(px(14.))
            .py(px(8.))
            .size_sm()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .max_w_full()
                    .child(exact_text("Added ").on_line(SM_FACE, &SM_MONO_LINE))
                    .children(words(added).into_iter().map(|word| {
                        word.mono().size_xs().on_line(SM_FACE, &SM_MONO_LINE)
                    }))
                    .children(
                        words(" to the ignore list. Matching files are hidden here now, and the next index does not read them.")
                            .into_iter()
                            .map(|word| word.on_line(SM_FACE, &SM_MONO_LINE)),
                    ),
            )
            .child(
                button::secondary("ignore-added-edit", "Edit the ignore list")
                    .on_click(cx.listener(|panel, _, _, cx| panel.open_setting("ignore", cx))),
            )
            .child(
                div()
                    .id("ignore-added-dismiss")
                    .test_support()
                    .role(Role::Button)
                    .aria_label("dismiss")
                    .flex_none()
                    .cursor_pointer()
                    .size_xs()
                    .semibold()
                    .text_color(t.accent)
                    .on_click(cx.listener(|panel, _, _, cx| {
                        panel.added = None;
                        cx.notify();
                    }))
                    .child(exact_text("dismiss")),
            )
    }

    fn group(
        &mut self,
        kind: SkipKind,
        items: Vec<Skip>,
        filtering: bool,
        t: &Tokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = kind_key(kind);
        let (title, explain) = kind_text(kind);
        let expanded = self.open.contains(&kind) || filtering;
        let total = items.len();
        let shown = if self.whole.contains(&kind) {
            total
        } else {
            total.min(SHOWN_PER_GROUP)
        };
        let group_key = format!("group:{key}");
        let group_editor = self.editor_for(&group_key);
        let edit_items = items.clone();
        let header = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(8.))
            .child(
                div()
                    .id(SharedString::from(format!("skip-group-{key}-toggle")))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(title)
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |panel, _, _, cx| panel.toggle_group(kind, cx)))
                    .child(chevron(expanded, t.muted))
                    .child(exact_text(title).semibold().size_sm())
                    .child(
                        div()
                            .flex_none()
                            .px(px(6.))
                            .rounded(px(4.))
                            .bg(t.sunken)
                            .size_sm()
                            .tabular()
                            .child(exact_text(total.to_string())),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .size_sm()
                            .text_color(t.muted)
                            .truncate()
                            .child(baseline::text(explain)),
                    ),
            )
            .when(kind == SkipKind::Scanned, |d| {
                d.child(
                    button::secondary("skip-group-scanned-ocr", "OCR settings")
                        .on_click(cx.listener(|panel, _, _, cx| panel.open_setting("pdf.ocr", cx))),
                )
            })
            .child(
                button::secondary(
                    SharedString::from(format!("skip-group-{key}-ignore")),
                    ignore_label(total),
                )
                .on_click(cx.listener(move |panel, _, window, cx| {
                    panel.edit(group_key.clone(), edit_items.clone(), window, cx)
                })),
            );
        let mut card = div()
            .id(SharedString::from(format!("skip-group-{key}")))
            .test_support()
            .aria_label(title)
            .relative()
            .border_1()
            .border_color(transparent_black())
            .rounded(px(6.))
            .min_w(px(0.))
            .flex()
            .flex_col()
            .child(css_box(transparent_black(), Some(t.line), 6.))
            .child(header);
        if let Some(editor) = group_editor {
            card = card.child(div().px(px(12.)).pb(px(12.)).child(editor));
        }
        if expanded {
            let mut rows = div()
                .id(SharedString::from(format!("skip-rows-{key}")))
                .flex()
                .flex_col()
                .px(px(12.))
                .pb(px(8.))
                .max_h(px(ROWS_MAX_HEIGHT))
                .overflow_y_scroll();
            for (ix, item) in items.iter().take(shown).enumerate() {
                rows = rows.child(self.row(kind, ix, item, t, window, cx));
                if let Some(editor) = self.editor_for(&item.path) {
                    rows = rows.child(div().pb(px(8.)).child(editor));
                }
            }
            if shown < total {
                rows = rows.child(
                    div()
                        .relative()
                        .border_t_1()
                        .border_color(transparent_black())
                        .child(css_rule_top(t.sunken))
                        .pt(px(8.))
                        .child(
                            div()
                                .id(SharedString::from(format!("skip-group-{key}-all")))
                                .test_support()
                                .role(Role::Button)
                                .aria_label(format!("Show all {total}"))
                                .cursor_pointer()
                                .size_xs()
                                .semibold()
                                .text_color(t.accent)
                                .on_click(cx.listener(move |panel, _, _, cx| {
                                    panel.whole.insert(kind);
                                    cx.notify();
                                }))
                                .child(exact_text(format!("Show all {total}"))),
                        ),
                );
            }
            card = card.child(rows);
        }
        card
    }

    fn row(
        &self,
        kind: SkipKind,
        ix: usize,
        item: &Skip,
        t: &Tokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = format!("skip-{}-{ix}", kind_key(kind));
        let key = row_key(item);
        let open = self.rows_open.contains(&key);
        let copied = self.copied.contains(&key);
        let (project, shown) = short_path(&item.path);
        let code = item.io.as_ref().map(|io| match io.os_error {
            Some(n) => format!("{} · os error {n}", io.kind),
            None => io.kind.clone(),
        });
        let toggle_key = key.clone();
        let copy_key = key.clone();
        let path = item.path.clone();
        let title = SharedString::from(item.path.clone());
        let one = vec![item.clone()];
        let edit_key = item.path.clone();
        let line =
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .py(px(6.))
                .min_w(px(0.))
                .size_sm()
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-toggle")))
                        .test_support()
                        .role(Role::Button)
                        .aria_label(item.path.clone())
                        .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .cursor_pointer()
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            panel.toggle_row(toggle_key.clone(), cx)
                        }))
                        .when_some(project, |d, project| {
                            d.child(
                                div()
                                    .flex_none()
                                    .max_w(relative(0.3))
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(t.sunken)
                                    .size_xs()
                                    .truncate()
                                    .child(baseline::text(project)),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_grow(1.)
                                .min_w(px(0.))
                                .mono()
                                .size_xs()
                                .child(on_web_baseline(
                                    tail_text(shown),
                                    XS_MONO_FACE,
                                    &[XS_MONO_FACE],
                                )),
                        )
                        .when_some(code, |d, code| {
                            d.child(
                                div()
                                    .flex_none()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(t.warn_soft)
                                    .text_color(t.warn)
                                    .mono()
                                    .text_size(px(11.))
                                    .line_height(px(CHIP_LINE))
                                    .child(exact_text(code)),
                            )
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-ignore")))
                        .test_support()
                        .role(Role::Button)
                        .aria_label("Ignore…")
                        .flex_none()
                        .cursor_pointer()
                        .size_xs()
                        .semibold()
                        .text_color(t.accent)
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.edit(edit_key.clone(), one.clone(), window, cx)
                        }))
                        .child(exact_text("Ignore…")),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-copy")))
                        .test_support()
                        .role(Role::Button)
                        .aria_label(if copied { "copied" } else { "copy path" })
                        .flex_none()
                        .cursor_pointer()
                        .size_xs()
                        .semibold()
                        .text_color(t.accent)
                        .on_click(cx.listener(move |panel, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                            panel.copied.insert(copy_key.clone());
                            cx.notify();
                        }))
                        .child(exact_text(if copied { "copied" } else { "copy path" })),
                );
        div()
            .relative()
            .border_t_1()
            .border_color(transparent_black())
            .child(css_rule_top(t.sunken))
            .min_w(px(0.))
            .flex()
            .flex_col()
            .child(line)
            .when(open, |d| d.child(details(item, t, window)))
    }
}

fn details(item: &Skip, t: &Tokens, window: &Window) -> impl IntoElement {
    let mut facts = vec![
        ("Path", item.path.clone()),
        (
            "Reason",
            if item.detail.is_empty() {
                "no detail recorded".to_string()
            } else {
                item.detail.clone()
            },
        ),
    ];
    if let Some(io) = &item.io {
        facts.push((
            "Error",
            match io.os_error {
                Some(n) => format!("{}, os error {n}", io.kind),
                None => io.kind.clone(),
            },
        ));
    }
    let width = facts
        .iter()
        .map(|(label, _)| label_width(window, label))
        .fold(px(0.), |a, b| a.max(b));
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .pb(px(8.))
        .pl(px(4.))
        .size_xs()
        .children(facts.into_iter().map(|(label, value)| {
            div()
                .flex()
                .gap(px(12.))
                .child(
                    div()
                        .w(width)
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(t.muted)
                        .child(baseline::text(label)),
                )
                .child(
                    div()
                        .flex_grow(1.)
                        .flex_basis(px(0.))
                        .min_w(px(0.))
                        .mono()
                        .child(baseline::text(value)),
                )
        }))
}

fn label_width(window: &Window, label: &str) -> Pixels {
    let run = TextRun {
        len: label.len(),
        font: Font {
            weight: FontWeight::NORMAL,
            ..font(SharedString::from(SANS))
        },
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .layout_line(label, px(12.), &[run], None)
        .width
}

impl Render for SkippedPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let aside = self.aside(&t, window, cx);
        let filtering = !self.filter_text.trim().is_empty();
        let groups = groups(&self.skipped, &self.filter_text);
        let mut body = panel("Skipped files", Some(aside), "skipped-panel");
        if let Some(added) = self.added.clone() {
            body = body.child(self.added_banner(&added, &t, cx));
        }
        if self.skipped.is_empty() {
            let line = empty_line(self.hidden);
            body = body.child(
                div()
                    .id("skipped-empty")
                    .test_support()
                    .aria_label(line.clone())
                    .size_sm()
                    .text_color(t.muted)
                    .child(baseline::text(line)),
            );
        } else if groups.is_empty() {
            let line = format!("No skipped file matches “{}”.", self.filter_text);
            body = body.child(
                div()
                    .id("skipped-no-match")
                    .test_support()
                    .aria_label(line.clone())
                    .size_sm()
                    .text_color(t.muted)
                    .child(baseline::text(line)),
            );
        }
        if !groups.is_empty() {
            let mut list = div().flex().flex_col().gap(px(8.));
            for (kind, items) in groups {
                list = list.child(self.group(kind, items, filtering, &t, window, cx));
            }
            body = body.child(list);
        }
        body
    }
}
