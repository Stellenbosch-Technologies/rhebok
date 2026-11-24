use std::time::Duration;

use gpui_kit::base::input::TextareaState;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::Value as Json;
use toml_edit::DocumentMut;

use super::fields::{bounds_recorder, card, Ctx, RING_RADIUS, RING_WIDTH};
use super::nav::field_id;
use super::pending::{js_number, js_order, toml_to_json, Pending};
use crate::theme::{tokens, Face, Type, MONO, SM_FACE};
use crate::widgets::css_box::css_ring;
use crate::widgets::inline_flow::{inline_flow, Chip, Segment};
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{button, text_area};

pub const COPIED_FOR: Duration = Duration::from_millis(1500);
pub const RAW_KEY: &str = "raw";
pub const AREA_ID: &str = "raw-config";
pub const AREA_LABEL: &str = "raw config.toml";
pub const TITLE: &str = "Raw config.toml";
pub const HINT: &str = "The keys this file sets, with your unsaved changes applied. Comments are not shown here and are kept on save. Apply turns your edits into unsaved changes; they are checked and saved like any other.";
pub const SYNTAX_ERROR: &str = "TOML syntax error: ";
pub const MIN_ROWS: usize = 8;
pub const MAX_ROWS: usize = 30;
pub const LINE_HEIGHT: f32 = 19.5;
pub const PAD_X: f32 = 12.;
pub const PAD_Y: f32 = 8.;
pub const BORDER: f32 = 1.;

const CODE_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 12., 20.);
const CODE_PAD: f32 = 4.;
const CODE_RADIUS: f32 = 4.;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawLocal {
    pub draft: Option<String>,
    pub problem: Option<String>,
    pub copied: bool,
    pub generation: u64,
}

impl RawLocal {
    pub fn reset(&mut self) {
        *self = RawLocal {
            generation: self.generation + 1,
            ..RawLocal::default()
        };
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Scalar(Json),
    Array(Vec<Node>),
    Table(Vec<(String, Node)>),
}

fn node_of(value: &Json, sorted: bool) -> Node {
    match value {
        Json::Array(items) => Node::Array(items.iter().map(|v| node_of(v, sorted)).collect()),
        Json::Object(map) => {
            let mut entries: Vec<(String, Node)> = map
                .iter()
                .map(|(k, v)| (k.clone(), node_of(v, sorted)))
                .collect();
            if sorted {
                entries.sort_by(|a, b| a.0.cmp(&b.0));
            }
            js_order(&mut entries);
            Node::Table(entries)
        }
        other => Node::Scalar(other.clone()),
    }
}

fn entry<'a>(entries: &'a mut [(String, Node)], key: &str) -> Option<&'a mut Node> {
    entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn assign(entries: &mut Vec<(String, Node)>, key: &str, value: Node) {
    match entry(entries, key) {
        Some(slot) => *slot = value,
        None => {
            entries.push((key.to_string(), value));
            js_order(entries);
        }
    }
}

fn apply(entries: &mut Vec<(String, Node)>, parts: &[&str], leaf: &str, value: Option<Node>) {
    let Some((segment, rest)) = parts.split_first() else {
        match value {
            None => entries.retain(|(k, _)| k != leaf),
            Some(value) => assign(entries, leaf, value),
        }
        return;
    };
    if !matches!(entry(entries, segment), Some(Node::Table(_))) {
        if value.is_none() {
            return;
        }
        assign(entries, segment, Node::Table(Vec::new()));
    }
    if let Some(Node::Table(next)) = entry(entries, segment) {
        apply(next, rest, leaf, value);
    }
}

fn with_pending(file: &Json, pending: &Pending) -> Node {
    let mut root = node_of(file, true);
    let Node::Table(entries) = &mut root else {
        return root;
    };
    for path in pending.paths() {
        let value = match pending.get(&path) {
            Some(Some(value)) => Some(node_of(&toml_to_json(value), false)),
            Some(None) => None,
            None => continue,
        };
        let mut parts: Vec<&str> = path.split('.').collect();
        let leaf = parts.pop().unwrap_or_default();
        apply(entries, &parts, leaf, value);
    }
    root
}

pub fn mirror(file: &Json, pending: &Pending) -> String {
    stringify(&with_pending(file, pending))
}

pub fn stringify_json(value: &Json) -> String {
    stringify(&node_of(value, false))
}

fn stringify(root: &Node) -> String {
    let text = match root {
        Node::Table(entries) => table(None, entries, ""),
        _ => String::new(),
    };
    if text.ends_with('\n') {
        text
    } else {
        text + "\n"
    }
}

fn bare(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn quoted(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_default()
        .replace('\u{7f}', "\\u007f")
}

fn key(k: &str) -> String {
    if bare(k) {
        k.to_string()
    } else {
        quoted(k)
    }
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.;

fn number(n: &serde_json::Number) -> String {
    let Some(f) = n.as_f64() else {
        return n.to_string();
    };
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0. { "inf" } else { "-inf" }.into();
    }
    if f == f.trunc() && f.abs() > MAX_SAFE_INTEGER && f.abs() < 1e21 {
        return format!("{}.0", f as i128);
    }
    js_number(n)
}

fn value(node: &Node) -> String {
    match node {
        Node::Scalar(Json::Number(n)) => number(n),
        Node::Scalar(Json::Bool(b)) => b.to_string(),
        Node::Scalar(Json::String(s)) => quoted(s),
        Node::Scalar(_) => String::new(),
        Node::Array(items) if items.is_empty() => "[]".into(),
        Node::Array(items) => format!(
            "[ {} ]",
            items.iter().map(value).collect::<Vec<_>>().join(", ")
        ),
        Node::Table(entries) if entries.is_empty() => "{}".into(),
        Node::Table(entries) => format!(
            "{{ {} }}",
            entries
                .iter()
                .map(|(k, v)| format!("{} = {}", key(k), value(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn array_of_tables(items: &[Node]) -> bool {
    !items.is_empty() && items.iter().all(|i| matches!(i, Node::Table(_)))
}

fn table(header: Option<&str>, entries: &[(String, Node)], prefix: &str) -> String {
    let mut preamble = String::new();
    let mut tables = String::new();
    let nested = |k: &str| {
        if prefix.is_empty() {
            k.to_string()
        } else {
            format!("{prefix}.{k}")
        }
    };
    for (k, v) in entries {
        if matches!(v, Node::Scalar(Json::Null)) {
            continue;
        }
        let k = key(k);
        let block = match v {
            Node::Array(items) if array_of_tables(items) => {
                let name = nested(&k);
                let mut out = String::new();
                for item in items {
                    if let Node::Table(inner) = item {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&format!("[[{name}]]\n"));
                        out.push_str(&table(None, inner, &name));
                    }
                }
                Some(out)
            }
            Node::Table(inner) => {
                let name = nested(&k);
                Some(table(Some(&name), inner, &name))
            }
            other => {
                preamble.push_str(&format!("{k} = {}\n", value(other)));
                None
            }
        };
        if let Some(block) = block {
            if !tables.is_empty() {
                tables.push('\n');
            }
            tables.push_str(&block);
        }
    }
    if let Some(header) = header {
        if !preamble.is_empty() || tables.is_empty() {
            preamble = if preamble.is_empty() {
                format!("[{header}]")
            } else {
                format!("[{header}]\n{preamble}")
            };
        }
    }
    if !preamble.is_empty() && !tables.is_empty() {
        format!("{preamble}\n{tables}")
    } else if preamble.is_empty() {
        tables
    } else {
        preamble
    }
}

pub fn rows(text: &str) -> usize {
    (text.split('\n').count() + 1).clamp(MIN_ROWS, MAX_ROWS)
}

pub fn height(rows: usize) -> f32 {
    rows as f32 * LINE_HEIGHT + 2. * (PAD_Y + BORDER)
}

fn lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

pub fn code_block(text: &str, at: usize) -> String {
    let before = &text[..at.min(text.len())];
    let leading = lines(before);
    let line = leading.len();
    let column = leading.last().map_or(0, |l| l.encode_utf16().count()) + 1;
    let all = lines(text);
    let width = ((line + 1) as f64).log10().floor() as usize + 1;
    let mut out = String::new();
    for i in line.saturating_sub(1)..=line + 1 {
        let Some(l) = i.checked_sub(1).and_then(|ix| all.get(ix)) else {
            continue;
        };
        if l.is_empty() {
            continue;
        }
        out.push_str(&format!("{i:<width$}:  {l}\n"));
        if i == line {
            out.push_str(&" ".repeat(width + column + 2));
            out.push_str("^\n");
        }
    }
    out
}

pub fn parse(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>().map_err(|e| {
        let at = e.span().map_or(0, |s| s.start);
        let message = e.message().trim_end().replace('\n', " ");
        format!(
            "Invalid TOML document: {message}\n\n{}",
            code_block(text, at)
        )
    })
}

pub fn shown_problem(problem: &str) -> String {
    format!("{SYNTAX_ERROR}{problem}")
}

pub struct RawView<'a> {
    pub local: &'a RawLocal,
    pub area: Option<&'a Entity<TextareaState>>,
    pub text: String,
}

fn unparseable(path: &str, cx: &App) -> AnyElement {
    let t = *tokens(cx);
    let chip = Some(Chip {
        background: t.sunken,
        pad: CODE_PAD,
        radius: CODE_RADIUS,
    });
    let text = format!("config.toml does not parse, so it cannot be edited here. Fix it by hand at {path}; the error is listed at the top of this page.");
    div()
        .id("raw-unparseable")
        .test_support()
        .aria_label(SharedString::from(text))
        .size_sm()
        .text_color(t.warn)
        .child(inline_flow(
            SM_FACE,
            vec![
                Segment::text(
                    "config.toml does not parse, so it cannot be edited here. Fix it by hand at ",
                    SM_FACE,
                ),
                Segment::chip(path.to_string(), CODE_FACE, chip),
                Segment::text("; the error is listed at the top of this page.", SM_FACE),
            ],
        ))
        .into_any_element()
}

pub fn raw_section(ctx: &Ctx, raw: RawView, cx: &App) -> AnyElement {
    let t = *tokens(cx);
    let view = ctx.view.clone();
    let copy = button::secondary("raw-copy", if raw.local.copied { "Copied" } else { "Copy" })
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |s, cx| s.copy_raw(cx));
        })
        .into_any_element();
    let mut body = card("card-raw", TITLE, Some(copy), cx);
    if ctx.state.unparseable() {
        body = body.child(unparseable(ctx.state.path(), cx));
    } else {
        let edited = raw.local.draft.is_some();
        let apply_view = ctx.view.clone();
        let revert_view = ctx.view.clone();
        body = body
            .child(
                div()
                    .size_sm()
                    .text_color(t.muted)
                    .child(wrapped_text(HINT)),
            )
            .when_some(raw.area, |d, area| {
                d.child(
                    text_area(AREA_ID, AREA_LABEL, area)
                        .h(px(height(rows(&raw.text))))
                        .px(px(PAD_X))
                        .py(px(PAD_Y))
                        .mono()
                        .text_size(px(12.))
                        .line_height(px(LINE_HEIGHT)),
                )
            })
            .when_some(raw.local.problem.clone(), |d, problem| {
                let shown = shown_problem(&problem);
                let collapsed = shown.split_whitespace().collect::<Vec<_>>().join(" ");
                d.child(
                    div()
                        .id("raw-problem")
                        .test_support()
                        .role(Role::Alert)
                        .aria_label(SharedString::from(shown))
                        .size_xs()
                        .text_color(t.warn)
                        .child(wrapped_text(collapsed)),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        button::primary("raw-apply", "Apply to form")
                            .disabled(!edited || ctx.locked)
                            .on_click(move |_, _, cx| {
                                let _ = apply_view.update(cx, |s, cx| s.apply_raw(cx));
                            }),
                    )
                    .child(
                        button::secondary("raw-revert", "Revert")
                            .disabled(!edited)
                            .on_click(move |_, _, cx| {
                                let _ = revert_view.update(cx, |s, cx| s.revert_raw(cx));
                            }),
                    ),
            );
    }
    let ringed = ctx.ring == Some(RAW_KEY);
    div()
        .id(field_id(RAW_KEY))
        .test_support()
        .aria_label(TITLE)
        .relative()
        .child(bounds_recorder(ctx, RAW_KEY))
        .child(body)
        .when(ringed, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(1.))
                    .left(px(1.))
                    .right(px(1.))
                    .bottom(px(1.))
                    .child(css_ring(t.warn, RING_WIDTH, RING_RADIUS + RING_WIDTH)),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{mirror, rows, stringify_json};
    use crate::tabs::settings::pending::Pending;

    #[test]
    fn the_mirror_writes_toml_the_way_smol_toml_stringifies_it() {
        let file = json!({
            "index_transcripts": false,
            "sources": ["/a", "b\"c"],
            "odd key": 1,
            "big": 1e21,
            "n": 2.5e-7,
            "embed": { "ollama_url": "http://x", "concurrency": 2 },
            "weights": { "transcript": 0.5, "decay": { "enabled": true, "floor": 0.1 } },
            "hook": { "threshold": 0.66, "weights": {} },
            "mcp": { "weights": { "pdf": 1 } },
            "items": [{ "a": 1 }, { "b": "x" }]
        });
        assert_eq!(
            stringify_json(&file),
            "index_transcripts = false\nsources = [ \"/a\", \"b\\\"c\" ]\n\"odd key\" = 1\nbig = 1e+21\nn = 2.5e-7\n\n[embed]\nollama_url = \"http://x\"\nconcurrency = 2\n\n[weights]\ntranscript = 0.5\n\n[weights.decay]\nenabled = true\nfloor = 0.1\n\n[hook]\nthreshold = 0.66\n\n[hook.weights]\n[mcp.weights]\npdf = 1\n\n[[items]]\na = 1\n\n[[items]]\nb = \"x\"\n",
            "smol-toml 1.9.1 printed exactly this for the same object"
        );
        assert_eq!(stringify_json(&json!({})), "\n");
        assert_eq!(stringify_json(&json!({ "a": {} })), "[a]\n");
        assert_eq!(
            stringify_json(&json!({ "a": { "b": { "c": 1 } } })),
            "[a.b]\nc = 1\n"
        );
    }

    #[test]
    fn the_mirror_sorts_the_file_and_appends_pending_keys_in_edit_order() {
        let file = json!({ "sources": ["/n"], "embed": { "ollama_url": "u" }, "index_transcripts": false });
        let mut pending = Pending::default();
        pending.put("index_transcripts", Some(toml::Value::Boolean(true)));
        pending.put("hook.threshold", Some(toml::Value::Float(0.5)));
        pending.put("embed.ollama_url", None);
        pending.put("ignore.0", None);
        assert_eq!(
            mirror(&file, &pending),
            "index_transcripts = true\nsources = [ \"/n\" ]\n\n[embed]\n[hook]\nthreshold = 0.5\n",
            "smol-toml prints the web's withPending of the same file and edits exactly so"
        );
    }

    #[test]
    fn integer_like_keys_come_first_in_numeric_order_as_in_a_js_object() {
        let file = json!({ "b": 1, "10": 2, "2": 3, "01": 4, "a": { "10": 1, "2": 2 } });
        assert_eq!(
            stringify_json(&file),
            "2 = 3\n10 = 2\nb = 1\n01 = 4\n\n[a]\n2 = 2\n10 = 1\n",
            "smol-toml 1.9.1 printed exactly this for the same object"
        );
        let file = json!({ "x": { "b": 1 }, "2": 5 });
        let mut pending = Pending::default();
        pending.put("10.c", Some(toml::Value::Integer(1)));
        assert_eq!(
            mirror(&file, &pending),
            "2 = 5\n\n[10]\nc = 1\n\n[x]\nb = 1\n",
            "a key added by an edit takes its integer slot, as JS assignment does"
        );
    }

    #[test]
    fn the_editor_is_eight_to_thirty_rows() {
        assert_eq!(rows("a = 1\n"), 8);
        assert_eq!(rows(&"a = 1\n".repeat(10)), 12);
        assert_eq!(rows(&"a = 1\n".repeat(40)), 30);
    }
}
