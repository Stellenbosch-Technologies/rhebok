use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::base::input::InputState;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::config::check::FieldError;
use serde_json::Value as Json;

use super::nav::{field_id, Section};
use super::pending::{describe, errors_for, ConfigState, Pending};
use super::Settings;
use crate::theme::{tokens, Face, Tokens, Type, SANS, XS_FACE, XS_MONO_FACE};
use crate::widgets::css_box::{css_box, css_px_box, css_ring, ExactSpan};
use crate::widgets::inline_flow::{inline_flow, Chip, Segment};
use crate::widgets::rich::Rich;
use crate::widgets::segmented::segmented;
use crate::widgets::select::{select, SelectSize};
use crate::widgets::tail::ellipsis_text;
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{
    exact_text, inline_line, text_button, text_input, tracked, ExactLink, Tracked,
};

pub const ALREADY_IN_FILE: &str = " (already in config.toml)";
pub const RING_WIDTH: f32 = 2.;
pub const RING_RADIUS: f32 = 6.;
pub const NOT_SET: &str = "not set";
pub const INPUT_PAD_X: f32 = 10.;

const LABEL_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 11., 16.5);
const RESET_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 11., 16.5);
const CODE_PAD: f32 = 4.;
const CODE_RADIUS: f32 = 4.;

#[derive(Debug, Default)]
pub struct BoundsMap {
    pub section: Option<Section>,
    pub fields: HashMap<String, (Bounds<Pixels>, Pixels)>,
}

pub type FieldBounds = Rc<RefCell<BoundsMap>>;

#[derive(Debug, Clone, PartialEq)]
pub enum InputKind {
    Number { optional: bool },
    Text { optional: bool },
    KeepAlive,
    Candidate,
    Draft,
}

#[derive(Debug, Clone)]
pub struct InputSpec {
    pub key: String,
    pub path: String,
    pub kind: InputKind,
    pub placeholder: String,
    pub disabled: bool,
    pub masked: bool,
}

impl InputSpec {
    pub fn number(path: &str, optional: bool, placeholder: Option<&str>) -> InputSpec {
        InputSpec {
            key: path.to_string(),
            path: path.to_string(),
            kind: InputKind::Number { optional },
            placeholder: placeholder
                .unwrap_or(if optional { NOT_SET } else { "" })
                .to_string(),
            disabled: false,
            masked: false,
        }
    }

    pub fn text(path: &str, optional: bool, placeholder: Option<&str>) -> InputSpec {
        InputSpec {
            key: path.to_string(),
            path: path.to_string(),
            kind: InputKind::Text { optional },
            placeholder: placeholder
                .unwrap_or(if optional { NOT_SET } else { "" })
                .to_string(),
            disabled: false,
            masked: false,
        }
    }

    pub fn keep_alive(path: &str) -> InputSpec {
        InputSpec {
            kind: InputKind::KeepAlive,
            ..InputSpec::text(path, false, None)
        }
    }

    pub fn draft(key: &str, placeholder: &str) -> InputSpec {
        InputSpec {
            kind: InputKind::Draft,
            ..InputSpec::text(key, false, Some(placeholder))
        }
    }

    pub fn masked(mut self) -> InputSpec {
        self.masked = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> InputSpec {
        self.disabled = disabled;
        self
    }

    pub fn candidate(path: &str, placeholder: &str) -> InputSpec {
        InputSpec {
            key: candidate_key(path),
            path: path.to_string(),
            kind: InputKind::Candidate,
            placeholder: placeholder.to_string(),
            disabled: false,
            masked: false,
        }
    }
}

pub fn candidate_key(path: &str) -> String {
    format!("add-{path}")
}

pub fn sync_placeholder(
    input: &Entity<InputState>,
    placeholder: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let wanted = SharedString::from(placeholder.to_string());
    input.update(cx, |state, cx| {
        if state.presentation().placeholder() != &wanted {
            state.set_placeholder(wanted, window, cx);
        }
    });
}

pub fn input_id(key: &str) -> SharedString {
    format!("input-{key}").into()
}

pub fn number_text(value: Option<&Json>) -> String {
    match value {
        None | Some(Json::Null) => String::new(),
        Some(Json::String(s)) => s.clone(),
        Some(v) => describe(Some(v)),
    }
}

pub fn parse_number(raw: &str, optional: bool) -> Option<toml::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (!optional).then(|| toml::Value::String(String::new()));
    }
    match trimmed.parse::<f64>() {
        Ok(n) if n.is_finite() && n == n.trunc() && n.abs() < 9.007_199_254_740_992e15 => {
            Some(toml::Value::Integer(n as i64))
        }
        Ok(n) if n.is_finite() => Some(toml::Value::Float(n)),
        _ => Some(toml::Value::String(trimmed.to_string())),
    }
}

pub fn parse_keep_alive(raw: &str) -> toml::Value {
    let trimmed = raw.trim();
    let digits = trimmed.strip_prefix('-').unwrap_or(trimmed);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Some(value) = parse_number(trimmed, false) {
            return value;
        }
    }
    toml::Value::String(trimmed.to_string())
}

pub struct Ctx<'a> {
    pub state: &'a ConfigState,
    pub pending: &'a Pending,
    pub errors: &'a [FieldError],
    pub blocking: &'a [FieldError],
    pub locked: bool,
    pub view: WeakEntity<Settings>,
    pub inputs: &'a HashMap<String, Entity<InputState>>,
    pub ring: Option<&'a str>,
    pub bounds: FieldBounds,
    pub scroll: ScrollHandle,
    pub section: Section,
}

impl Ctx<'_> {
    pub fn value(&self, path: &str) -> Option<Json> {
        self.pending.value_of(self.state, path)
    }

    pub fn setter(
        &self,
        path: &str,
        value: Option<toml::Value>,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let view = self.view.clone();
        let path = path.to_string();
        move |_, cx| {
            let value = value.clone();
            let _ = view.update(cx, |settings, cx| settings.set(&path, value, cx));
        }
    }

    pub fn input(&self, key: &str) -> Option<&Entity<InputState>> {
        self.inputs.get(key)
    }
}

pub enum Part {
    Text(&'static str),
    Code(&'static str),
}

pub enum Hint {
    Text(SharedString),
    Parts(Vec<Part>),
}

impl From<&'static str> for Hint {
    fn from(text: &'static str) -> Hint {
        Hint::Text(text.into())
    }
}

pub struct Field<'a> {
    pub path: &'a str,
    pub label: &'a str,
    pub hint: Option<Hint>,
    pub override_by: Option<String>,
    pub show_default: bool,
}

impl<'a> Field<'a> {
    pub fn new(path: &'a str, label: &'a str) -> Field<'a> {
        Field {
            path,
            label,
            hint: None,
            override_by: None,
            show_default: true,
        }
    }

    pub fn hint(mut self, hint: impl Into<Hint>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn without_default(mut self) -> Self {
        self.show_default = false;
        self
    }
}

pub fn code_line(
    parts: &[Part],
    strut: Face,
    code_face: Face,
    background: Option<Hsla>,
    _cx: &App,
) -> Div {
    let chip = background.map(|background| Chip {
        background,
        pad: CODE_PAD,
        radius: CODE_RADIUS,
    });
    let segments = parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => Segment::text(*text, strut),
            Part::Code(code) => Segment::chip(*code, code_face, chip),
        })
        .collect();
    div().child(inline_flow(strut, segments))
}

fn hint_element(hint: Hint, t: &Tokens, cx: &App) -> AnyElement {
    match hint {
        Hint::Text(text) => div()
            .size_xs()
            .text_color(t.muted)
            .child(wrapped_text(text))
            .into_any_element(),
        Hint::Parts(parts) => code_line(&parts, XS_FACE, XS_MONO_FACE, Some(t.sunken), cx)
            .size_xs()
            .text_color(t.muted)
            .into_any_element(),
    }
}

pub fn field_errors(ctx: &Ctx, path: &str, t: &Tokens, cx: &App) -> Option<AnyElement> {
    error_list(path, &errors_for(ctx.errors, path), ctx.blocking, t, cx)
}

pub fn error_list(
    path: &str,
    errors: &[&FieldError],
    blocking: &[FieldError],
    t: &Tokens,
    cx: &App,
) -> Option<AnyElement> {
    if errors.is_empty() {
        return None;
    }
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(errors.iter().enumerate().map(|(ix, e)| {
                let introduced = blocking.iter().any(|b| b.same_problem(e));
                let row = div()
                    .id(SharedString::from(format!("{}-error-{ix}", field_id(path))))
                    .test_support()
                    .role(Role::Alert)
                    .aria_label(SharedString::from(e.message.clone()))
                    .size_xs()
                    .text_color(t.warn);
                if introduced {
                    row.child(wrapped_text(e.message.clone()))
                        .into_any_element()
                } else {
                    let rich = Rich::new()
                        .push(&e.message, SANS, FontWeight::NORMAL, t.warn)
                        .push(ALREADY_IN_FILE, SANS, FontWeight::NORMAL, t.muted);
                    row.child(inline_line(cx, XS_FACE, &[XS_FACE], rich.styled()))
                        .into_any_element()
                }
            }))
            .into_any_element(),
    )
}

pub fn bounds_recorder(ctx: &Ctx, path: &str) -> impl IntoElement {
    let map = ctx.bounds.clone();
    let scroll = ctx.scroll.clone();
    let path = path.to_string();
    canvas(
        move |bounds, _, _| {
            map.borrow_mut()
                .fields
                .insert(path, (bounds, scroll.offset().y));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

pub fn section_marker(bounds: FieldBounds, section: Section) -> impl IntoElement {
    canvas(
        move |_, _, _| {
            let mut map = bounds.borrow_mut();
            map.section = Some(section);
            map.fields.clear();
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

pub const LABEL_GAP: f32 = 8.;

pub fn label(text: &str) -> Tracked {
    tracked(
        text.to_uppercase(),
        SANS,
        FontWeight::SEMIBOLD,
        px(LABEL_FACE.size),
        0.05,
    )
    .line_height(px(LABEL_FACE.line_height))
}

pub fn grid(children: Vec<AnyElement>) -> Div {
    let mut rows = Vec::new();
    let mut cells = children.into_iter().peekable();
    while let Some(first) = cells.next() {
        let second = cells.next();
        rows.push(
            div()
                .flex()
                .gap(px(24.))
                .child(div().flex_1().min_w(px(0.)).child(first))
                .child(div().flex_1().min_w(px(0.)).children(second)),
        );
    }
    div().flex().flex_col().gap(px(16.)).children(rows)
}

pub fn label_and_default_tag(label: &str, t: &Tokens) -> (Tracked, Tracked) {
    let link = ExactLink::new();
    let label = tracked(
        label.to_uppercase(),
        SANS,
        FontWeight::SEMIBOLD,
        px(LABEL_FACE.size),
        0.05,
    )
    .line_height(px(LABEL_FACE.line_height))
    .ends(&link);
    let tag = tracked("default", SANS, FontWeight::NORMAL, px(10.), 0.)
        .line_height(px(15.))
        .framed(t.sunken, 4., 6., 0.)
        .after(&link, px(LABEL_GAP));
    (label, tag)
}

pub fn shell(ctx: &Ctx, field: Field, control: AnyElement, cx: &App) -> AnyElement {
    let t = *tokens(cx);
    let path = field.path;
    let at_default = ctx.pending.is_default(ctx.state, path);
    let changed = ctx.pending.contains(path);
    let id = field_id(path);
    let default_text = format!("default {}", describe(ctx.state.default_of(path)));
    let ringed = ctx.ring == Some(path);
    let reset = ctx.setter(path, None);
    let overridden = field.override_by.clone();
    let (label, default_tag) = label_and_default_tag(field.label, &t);
    div()
        .id(id.clone())
        .test_support()
        .aria_label(SharedString::from(field.label.to_string()))
        .relative()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(bounds_recorder(ctx, path))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(20.))
                .child(label.text_color(t.muted))
                .when(changed, |d| {
                    d.child(
                        div()
                            .id(SharedString::from(format!("{id}-edited")))
                            .test_support()
                            .aria_label("edited")
                            .child(
                                exact_text("edited")
                                    .semibold()
                                    .text_size(px(10.))
                                    .line_height(px(15.))
                                    .text_color(t.accent),
                            ),
                    )
                })
                .when(at_default && !changed, |d| {
                    d.child(
                        div()
                            .id(SharedString::from(format!("{id}-default")))
                            .test_support()
                            .aria_label("default")
                            .child(default_tag.text_color(t.muted)),
                    )
                })
                .when_some(overridden.clone(), |d, variable| {
                    d.child(
                        exact_text(format!("overridden by ${variable}"))
                            .semibold()
                            .text_size(px(10.))
                            .line_height(px(15.))
                            .text_color(t.warn),
                    )
                })
                .child(div().flex_grow(1.))
                .when(field.show_default, |d| {
                    d.child(
                        div()
                            .id(SharedString::from(format!("{id}-shipped")))
                            .test_support()
                            .aria_label(SharedString::from(default_text.clone()))
                            .flex()
                            .min_w(px(0.))
                            .max_w(px(260.))
                            .overflow_hidden()
                            .mono()
                            .text_size(px(11.))
                            .line_height(px(16.5))
                            .text_color(t.muted)
                            .child(ellipsis_text(default_text.clone())),
                    )
                })
                .when(!at_default && overridden.is_none(), |d| {
                    d.child(
                        text_button(
                            SharedString::from(format!("{id}-reset")),
                            "reset to default",
                            RESET_FACE,
                            t.accent,
                        )
                        .underline()
                        .disabled(ctx.locked)
                        .on_click(move |_, window, cx| reset(window, cx)),
                    )
                }),
        )
        .child(control)
        .when_some(field.hint, |d, hint| d.child(hint_element(hint, &t, cx)))
        .when_some(field_errors(ctx, path, &t, cx), |d, errors| d.child(errors))
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

pub fn card(id: impl Into<ElementId>, title: &str, aside: Option<AnyElement>, cx: &App) -> Div {
    let t = *tokens(cx);
    let id = id.into();
    div()
        .relative()
        .flex()
        .flex_col()
        .gap(px(16.))
        .p(px(20.))
        .border_1()
        .border_color(transparent_black())
        .child(css_box(t.surface, Some(t.line), 8.))
        .child(
            div()
                .id(id)
                .test_support()
                .aria_label(SharedString::from(title.to_string()))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .semibold()
                        .size_sm()
                        .child(wrapped_text(title.to_string())),
                )
                .children(aside),
        )
}

pub fn toggle(
    id: impl Into<ElementId>,
    label: &str,
    checked: bool,
    disabled: bool,
    on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
    t: &Tokens,
) -> impl IntoElement {
    let knob = Hsla::from(rgba(0x0000001a));
    div()
        .id(id.into())
        .test_support()
        .role(Role::Switch)
        .aria_label(SharedString::from(label.to_string()))
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .relative()
        .w(px(36.))
        .h(px(20.))
        .rounded_full()
        .when(disabled, |d| d.opacity(0.5).cursor_not_allowed())
        .when(!disabled, |d| {
            d.cursor_pointer()
                .on_click(move |_, window, cx| on_toggle(!checked, window, cx))
        })
        .child(css_box(if checked { t.accent } else { t.line }, None, 10.))
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if checked { 18. } else { 2. }))
                .size(px(16.))
                .rounded_full()
                .bg(t.surface)
                .shadow(vec![
                    BoxShadow {
                        color: knob,
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(3.),
                        spread_radius: px(0.),
                        inset: false,
                    },
                    BoxShadow {
                        color: knob,
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(2.),
                        spread_radius: px(-1.),
                        inset: false,
                    },
                ]),
        )
}

pub fn toggle_field(
    ctx: &Ctx,
    path: &str,
    label: &str,
    hint: Option<Hint>,
    cx: &App,
) -> AnyElement {
    let t = *tokens(cx);
    let on = ctx.value(path) == Some(Json::Bool(true));
    let view = ctx.view.clone();
    let key = path.to_string();
    let control = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(toggle(
            SharedString::from(format!("toggle-{path}")),
            path,
            on,
            ctx.locked,
            move |next, _, cx| {
                let _ = view.update(cx, |s, cx| {
                    s.set(&key, Some(toml::Value::Boolean(next)), cx)
                });
            },
            &t,
        ))
        .child(exact_text(if on { "on" } else { "off" }).size_sm())
        .into_any_element();
    let mut field = Field::new(path, label);
    field.hint = hint;
    shell(ctx, field, control, cx)
}

pub fn number_input(
    ctx: &Ctx,
    path: &str,
    width: f32,
    frame_span: Option<ExactSpan>,
) -> AnyElement {
    match ctx.input(path) {
        Some(state) => text_input(input_id(path), path.to_string(), state)
            .frame_span(frame_span)
            .w(px(width))
            .px(px(10.))
            .py(px(6.))
            .mono()
            .size_sm()
            .when(ctx.locked, |d| d.opacity(0.5))
            .into_any_element(),
        None => div().h(px(34.)).into_any_element(),
    }
}

pub fn number_field(
    ctx: &Ctx,
    path: &str,
    label: &str,
    hint: Option<Hint>,
    cx: &App,
) -> AnyElement {
    let control = number_input(ctx, path, 140., None);
    let mut field = Field::new(path, label);
    field.hint = hint;
    shell(ctx, field, control, cx)
}

pub fn text_field(ctx: &Ctx, field: Field, mono: bool, cx: &App) -> AnyElement {
    let path = field.path;
    let disabled = ctx.locked || field.override_by.is_some();
    let control = match ctx.input(path) {
        Some(state) => text_input(input_id(path), path.to_string(), state)
            .px(px(10.))
            .py(px(6.))
            .map(|d| if mono { d.mono() } else { d.regular() })
            .size_sm()
            .when(disabled, |d| d.opacity(0.5))
            .into_any_element(),
        None => div().h(px(34.)).into_any_element(),
    };
    shell(ctx, field, control, cx)
}

pub fn select_field(
    ctx: &Ctx,
    field: Field,
    options: &[&str],
    optional: Option<&str>,
    cx: &App,
) -> AnyElement {
    let path = field.path.to_string();
    let current = match ctx.value(&path) {
        None | Some(Json::Null) => String::new(),
        Some(Json::String(s)) => s,
        Some(v) => describe(Some(&v)),
    };
    let mut values: Vec<String> = Vec::new();
    let mut labels: Vec<SharedString> = Vec::new();
    if let Some(empty) = optional {
        values.push(String::new());
        labels.push(empty.to_string().into());
    }
    for option in options {
        values.push(option.to_string());
        labels.push(option.to_string().into());
    }
    let selected = values.iter().position(|v| *v == current).unwrap_or(0);
    let view = ctx.view.clone();
    let key = path.clone();
    let control = select(
        format!("select-{path}"),
        labels,
        selected,
        SelectSize::Sm,
        move |ix, _, cx| {
            let value = values
                .get(ix)
                .filter(|v| !v.is_empty())
                .map(|v| toml::Value::String(v.clone()));
            let _ = view.update(cx, |s, cx| s.set(&key, value, cx));
        },
    )
    .disabled(ctx.locked)
    .pad_x(INPUT_PAD_X)
    .w(px(220.))
    .into_any_element();
    shell(ctx, field, control, cx)
}

pub const SLIDER_MARGIN: f32 = 1.;
pub const SLIDER_THUMB: f32 = 15.;
pub const SLIDER_HEIGHT: f32 = 16.;
pub const SLIDER_TRACK: f32 = 8.;
pub const SLIDER_STEPS: f64 = 100.;
pub const SLIDER_PAGE: f64 = 0.1;
const TRACK_FILL_EDGE: u32 = 0x875845;
const TRACK_EMPTY: u32 = 0xEFEFEF;
const TRACK_EMPTY_EDGE: u32 = 0xB2B2B2;

pub fn step_text(value: f64) -> String {
    let steps = (value.clamp(0., 1.) * SLIDER_STEPS).round();
    (steps / SLIDER_STEPS).to_string()
}

pub fn slider_fraction(value: Option<&Json>) -> f64 {
    let raw = value.and_then(Json::as_f64).unwrap_or(0.);
    (raw.clamp(0., 1.) * SLIDER_STEPS).round() / SLIDER_STEPS
}

pub fn keyed_fraction(key: &str, fraction: f64) -> Option<f64> {
    let step = 1. / SLIDER_STEPS;
    let next = match key {
        "right" | "up" => fraction + step,
        "left" | "down" => fraction - step,
        "pageup" => fraction + SLIDER_PAGE,
        "pagedown" => fraction - SLIDER_PAGE,
        "home" => 0.,
        "end" => 1.,
        _ => return None,
    };
    Some(next.clamp(0., 1.))
}

fn thumb_centre(bounds: Bounds<Pixels>, fraction: f64) -> f32 {
    let left = f32::from(bounds.left()) + SLIDER_MARGIN;
    let travel = f32::from(bounds.size.width) - 2. * SLIDER_MARGIN - SLIDER_THUMB;
    left + SLIDER_THUMB / 2. + fraction as f32 * travel
}

fn fraction_at(bounds: Bounds<Pixels>, x: f32) -> f64 {
    let left = f32::from(bounds.left()) + SLIDER_MARGIN + SLIDER_THUMB / 2.;
    let travel = f32::from(bounds.size.width) - 2. * SLIDER_MARGIN - SLIDER_THUMB;
    if travel <= 0. {
        return 0.;
    }
    f64::from(((x - left) / travel).clamp(0., 1.))
}

#[derive(Debug, Clone, Copy, Default)]
struct SliderDrag {
    bounds: Option<Bounds<Pixels>>,
    grab: Option<f32>,
}

type OnSlide = Rc<dyn Fn(String, &mut Window, &mut App)>;

fn paint_slider(bounds: Bounds<Pixels>, fraction: f64, t: &Tokens, window: &mut Window) {
    let bounds = css_px_box(bounds);
    let left = f32::from(bounds.left()) + SLIDER_MARGIN;
    let right = f32::from(bounds.right()) - SLIDER_MARGIN;
    let middle = f32::from(bounds.top()) + f32::from(bounds.size.height) / 2.;
    let top = middle - SLIDER_TRACK / 2.;
    let centre = thumb_centre(bounds, fraction);
    let radius = SLIDER_TRACK / 2.;
    let rect = |from: f32, to: f32, y: f32, h: f32| Bounds {
        origin: point(px(from), px(y)),
        size: size(px(to - from), px(h)),
    };
    window.paint_quad(quad(
        rect(left, right, top, SLIDER_TRACK),
        px(radius),
        Hsla::from(rgb(TRACK_EMPTY)),
        px(1.),
        Hsla::from(rgb(TRACK_EMPTY_EDGE)),
        BorderStyle::Solid,
    ));
    let mut filled = quad(
        rect(left, centre.max(left + radius), top, SLIDER_TRACK),
        Corners {
            top_left: px(radius),
            bottom_left: px(radius),
            top_right: px(0.),
            bottom_right: px(0.),
        },
        t.accent,
        Edges {
            top: px(1.),
            bottom: px(1.),
            left: px(1.),
            right: px(0.),
        },
        Hsla::from(rgb(TRACK_FILL_EDGE)),
        BorderStyle::Solid,
    );
    filled.border_style = BorderStyle::Solid;
    window.paint_quad(filled);
    window.paint_quad(quad(
        rect(
            centre - SLIDER_THUMB / 2.,
            centre + SLIDER_THUMB / 2.,
            middle - SLIDER_THUMB / 2.,
            SLIDER_THUMB,
        ),
        px(SLIDER_THUMB / 2.),
        t.accent,
        px(0.),
        transparent_black(),
        BorderStyle::Solid,
    ));
}

#[derive(IntoElement)]
pub struct RangeSlider {
    id: SharedString,
    fraction: f64,
    disabled: bool,
    on_change: OnSlide,
}

pub fn range_slider(
    id: impl Into<SharedString>,
    fraction: f64,
    disabled: bool,
    on_change: impl Fn(String, &mut Window, &mut App) + 'static,
) -> RangeSlider {
    RangeSlider {
        id: id.into(),
        fraction,
        disabled,
        on_change: Rc::new(on_change),
    }
}

impl RenderOnce for RangeSlider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        slider_element(
            self.id,
            self.fraction,
            self.disabled,
            self.on_change,
            window,
            cx,
        )
    }
}

fn slider_element(
    id: SharedString,
    fraction: f64,
    disabled: bool,
    on_change: OnSlide,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let t = *tokens(cx);
    let drag = window.use_keyed_state(ElementId::Name(format!("{id}-drag").into()), cx, |_, _| {
        SliderDrag::default()
    });
    let slide = move |drag: &Entity<SliderDrag>,
                      x: f32,
                      on_change: &OnSlide,
                      window: &mut Window,
                      cx: &mut App| {
        let state = *drag.read(cx);
        if let (Some(bounds), Some(grab)) = (state.bounds, state.grab) {
            let next = step_text(fraction_at(bounds, x - grab));
            if next != step_text(fraction) {
                on_change(next, window, cx);
            }
        }
    };
    let focus = window
        .use_keyed_state(
            ElementId::Name(format!("{id}-focus").into()),
            cx,
            |_, cx| cx.focus_handle().tab_stop(true),
        )
        .read(cx)
        .clone();
    let ringed = !disabled && focus.is_focused(window) && window.last_input_was_keyboard();
    let pressed = drag.clone();
    let press_change = on_change.clone();
    let key_change = on_change.clone();
    let tracker = drag.clone();
    let listener = drag.clone();
    let grab_focus = focus.clone();
    div()
        .id(ElementId::Name(id.clone()))
        .track_focus(&focus)
        .test_support()
        .role(Role::Slider)
        .aria_label(SharedString::from(format!("{id} slider")))
        .aria_value(SharedString::from(step_text(fraction)))
        .relative()
        .flex_grow(1.)
        .min_w(px(0.))
        .h(px(SLIDER_HEIGHT))
        .when(disabled, |d| d.opacity(0.5).cursor_not_allowed())
        .when(!disabled, |d| {
            d.on_key_down(move |event: &KeyDownEvent, window, cx| {
                if let Some(next) = keyed_fraction(&event.keystroke.key, fraction) {
                    cx.stop_propagation();
                    if step_text(next) != step_text(fraction) {
                        key_change(step_text(next), window, cx);
                    }
                }
            })
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                window.focus(&grab_focus, cx);
                let Some(bounds) = pressed.read(cx).bounds else {
                    return;
                };
                let x = f32::from(event.position.x);
                let centre = thumb_centre(bounds, fraction);
                let grab = if (x - centre).abs() <= SLIDER_THUMB / 2. {
                    x - centre
                } else {
                    0.
                };
                pressed.update(cx, |d, _| d.grab = Some(grab));
                slide(&pressed, x, &press_change, window, cx);
            })
        })
        .child(
            canvas(
                move |bounds, _, cx| {
                    tracker.update(cx, |d, _| d.bounds = Some(bounds));
                },
                move |bounds, _, window, cx| {
                    paint_slider(bounds, fraction, &t, window);
                    if listener.read(cx).grab.is_none() {
                        return;
                    }
                    let moving = listener.clone();
                    let change = on_change.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && event.dragging() {
                            slide(&moving, f32::from(event.position.x), &change, window, cx);
                        }
                    });
                    let releasing = listener.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            releasing.update(cx, |d, _| d.grab = None);
                        }
                    });
                },
            )
            .absolute()
            .inset_0(),
        )
        .when(ringed, |d| {
            d.child(
                div()
                    .absolute()
                    .inset(px(1.))
                    .child(css_ring(t.accent, RING_WIDTH, 0.)),
            )
        })
}

pub fn segmented_field(
    ctx: &Ctx,
    field: Field,
    options: &[(toml::Value, &str)],
    cx: &App,
) -> AnyElement {
    let path = field.path.to_string();
    let current = ctx.value(&path);
    let selected = options
        .iter()
        .position(|(v, _)| {
            current
                .as_ref()
                .is_some_and(|c| super::pending::same(c, &super::pending::toml_to_json(v)))
        })
        .unwrap_or(usize::MAX);
    let labels: Vec<&str> = options.iter().map(|(_, label)| *label).collect();
    let values: Vec<toml::Value> = options.iter().map(|(v, _)| v.clone()).collect();
    let view = ctx.view.clone();
    let key = path.clone();
    let control = div()
        .flex()
        .child(
            segmented(
                format!("segmented-{path}"),
                &labels,
                selected,
                move |ix, _, cx| {
                    let value = values.get(ix).cloned();
                    let _ = view.update(cx, |s, cx| s.set(&key, value, cx));
                },
            )
            .disabled(ctx.locked),
        )
        .into_any_element();
    shell(ctx, field, control, cx)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{div, px, Context, IntoElement, ParentElement, Render, Styled, Window};
    use serde_json::json;

    use super::{label_and_default_tag, number_text, parse_number, LABEL_GAP};
    use crate::widgets::button::tests::painted_span;

    struct LabelRow(&'static str);

    impl Render for LabelRow {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let t = *crate::theme::tokens(cx);
            let (label, tag) = label_and_default_tag(self.0, &t);
            div()
                .flex()
                .items_center()
                .gap(px(LABEL_GAP))
                .min_h(px(20.))
                .pl(px(263.))
                .child(label)
                .child(tag)
        }
    }

    #[gpui_kit::test]
    fn a_default_tag_paints_where_chrome_paints_it(_cx: &mut gpui_kit::TestAppContext) {
        for (label, chrome) in [
            ("IGNORE LIST", (674., 756.)),
            ("INDEX CODEX SESSIONS", (790., 874.)),
            ("MAX AGE IN DAYS", (718., 802.)),
        ] {
            assert_eq!(
                painted_span(1400., move || LabelRow(label), |t| t.sunken),
                chrome,
                "{label}: Chrome lays the tag out after a 0.05em-tracked label and paints these device px"
            );
        }
    }

    #[test]
    fn a_number_input_keeps_what_it_cannot_parse() {
        assert_eq!(parse_number("", true), None);
        assert_eq!(
            parse_number(" ", false),
            Some(toml::Value::String(String::new()))
        );
        assert_eq!(parse_number("30", true), Some(toml::Value::Integer(30)));
        assert_eq!(parse_number("0.5", false), Some(toml::Value::Float(0.5)));
        assert_eq!(
            parse_number("3x", false),
            Some(toml::Value::String("3x".into()))
        );
        assert_eq!(number_text(Some(&json!(null))), "");
        assert_eq!(number_text(Some(&json!(1.0))), "1");
        assert_eq!(number_text(Some(&json!("3x"))), "3x");
    }
}
