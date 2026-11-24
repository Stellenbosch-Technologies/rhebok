use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::config::check::FieldError;
use rhebok::dashboard::core::embed::EmbedTest;
use serde_json::Value as Json;

use super::fields::{
    card, error_list, grid, input_id, label, number_field, number_input, select_field, shell,
    text_field, toggle_field, Ctx, Field, Hint, InputSpec, INPUT_PAD_X,
};
use super::pending::{js_number, ConfigState};
use super::Settings;
use crate::theme::{tokens, Face, Tokens, Type, MONO, SANS, SM_FACE};
use crate::widgets::css_box::css_box;
use crate::widgets::inline_flow::{inline_flow, Chip, Segment};
use crate::widgets::segmented::segmented;
use crate::widgets::select::{select, SelectSize};
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{button, exact_text, text_button, text_input};

pub const URL: &str = "endpoint.url";
pub const MODEL: &str = "endpoint.model";
pub const TOKEN: &str = "embed.token";
pub const SAVED: &str = "Endpoint saved to the env file beside config.toml.";
pub const TOKEN_REMOVED: &str = "Token removed.";
pub const WRITE_ONLY: &str = "Write-only: the dashboard never shows a saved token.";
pub const WIRE_WARNING: &str = "If this serves different weights than the ones the index was built with, run a full re-index after saving.";
pub const SWITCH_NOTE: &str = "A remote endpoint is in use. Switching removes its URL from the env file; the model and token stay.";
pub const REINDEX_WARNING: &str = "Changing the embedding model or its dimensions makes the existing index unreadable: every document has to be embedded again with rhebok index --reindex. You can start that from here after saving.";
pub const REMOTE_MODEL_HINT: &str =
    "The name the index is stamped with; the endpoint's wire name is set above.";
pub const KEEP_ALIVE_HINT: &str = "Seconds, or an Ollama duration such as 30m.";
pub const CUSTOM_MODEL: &str =
    "A model rhebok has no presets for. Set its dimensions, prefix scheme and both relevance gates yourself.";
pub const PREFIX_SCHEMES: [&str; 5] = ["qwen3", "nomic", "e5", "gemma", "plain"];

const OVERRIDE_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 10., 15.);
const CANCEL_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 12., 16.);
const CODE_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 12., 20.);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ollama,
    Remote,
}

impl Mode {
    pub fn of(state: &ConfigState) -> Mode {
        if state.remote() {
            Mode::Remote
        } else {
            Mode::Ollama
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EmbedLocal {
    pub mode: Option<Mode>,
    pub seen_backend: Option<Mode>,
    pub seen_endpoint: Option<(Option<String>, Option<String>)>,
    pub replacing: bool,
    pub errors: Vec<FieldError>,
    pub saved: Option<String>,
    pub switching: bool,
    pub testing: bool,
    pub test: Option<EmbedTest>,
}

impl EmbedLocal {
    pub fn mode(&self, state: &ConfigState) -> Mode {
        self.mode.unwrap_or_else(|| Mode::of(state))
    }

    pub fn forget_remote(&mut self) {
        self.replacing = false;
        self.errors.clear();
        self.saved = None;
    }
}

pub fn draft_initial(key: &str, state: &ConfigState) -> String {
    match key {
        URL => state.endpoint("url").unwrap_or_default(),
        MODEL => state.endpoint("model").unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn inputs(state: &ConfigState, local: &EmbedLocal) -> Vec<InputSpec> {
    let mode = local.mode(state);
    let mut out = Vec::new();
    if mode == Mode::Remote {
        out.push(
            InputSpec::draft(URL, "https://host/v1").disabled(state.override_for(URL).is_some()),
        );
        out.push(
            InputSpec::draft(MODEL, "text-embedding-model")
                .disabled(state.override_for(MODEL).is_some()),
        );
        if local.replacing {
            out.push(InputSpec::draft(TOKEN, "new token").masked());
        }
    } else {
        out.push(InputSpec::text("embed.ollama_url", false, None));
        out.push(InputSpec::keep_alive("embed.keep_alive"));
    }
    out.push(InputSpec::text("embed.model", false, None));
    out.push(InputSpec::number("embed.dimensions", false, None));
    for path in ["embed.concurrency", "embed.batch", "embed.chunk_tokens"] {
        out.push(InputSpec::number(path, false, None));
    }
    out.push(InputSpec::text("embed.enrich_model", false, None));
    out
}

pub fn result_text(test: &EmbedTest, expected: Option<u64>) -> (String, Option<String>) {
    if !test.ok {
        return (
            format!(
                "Could not embed through {}: {}",
                test.url.as_deref().unwrap_or(test.backend),
                test.error.as_deref().unwrap_or("unknown error")
            ),
            None,
        );
    }
    let place = if test.backend == "remote" {
        "the remote endpoint"
    } else {
        "Ollama"
    };
    let dimensions = test
        .dimensions
        .map_or("null".to_string(), |d| d.to_string());
    let mut text = format!(
        "Connected to {place} in {} ms · {dimensions} dimensions",
        test.latency_ms
    );
    if let Some(model) = test.model.as_deref().filter(|m| !m.is_empty()) {
        text.push_str(&format!(" · {model}"));
    }
    let mismatch = match (expected, test.dimensions) {
        (Some(expected), Some(got)) if got as u64 != expected => Some(format!(
            " · the index expects {expected} dimensions, so this model needs a full re-index"
        )),
        _ => None,
    };
    (text, mismatch)
}

pub fn choice_summary(choice: &Json) -> String {
    let number = |key: &str| match &choice[key] {
        Json::Number(n) => js_number(n),
        _ => String::new(),
    };
    let mut text = format!(
        "{}. Choosing a model also sets its dimensions and both relevance gates (hook {}, MCP {}), because each model scores on its own scale.",
        choice["summary"].as_str().unwrap_or_default(),
        number("hook_threshold"),
        number("mcp_threshold"),
    );
    if let Some(min) = choice["min_ollama"].as_str() {
        text.push_str(&format!(
            " It needs Ollama {min} or newer; install checks the version before pulling it."
        ));
    }
    text
}

fn update(
    ctx: &Ctx,
    f: impl Fn(&mut Settings, &mut Context<Settings>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let view = ctx.view.clone();
    move |_, cx| {
        let _ = view.update(cx, |s, cx| f(s, cx));
    }
}

fn override_text(id: &str, variable: String, t: &Tokens) -> AnyElement {
    div()
        .id(SharedString::from(format!("{id}-override")))
        .test_support()
        .aria_label(SharedString::from(format!("overridden by ${variable}")))
        .child(
            exact_text(format!("overridden by ${variable}"))
                .semibold()
                .text_size(px(OVERRIDE_FACE.size))
                .line_height(px(OVERRIDE_FACE.line_height))
                .text_color(t.warn),
        )
        .into_any_element()
}

fn draft_input(ctx: &Ctx, key: &str, grow: bool) -> AnyElement {
    let disabled = ctx.locked || ctx.state.override_for(key).is_some();
    match ctx.input(key) {
        Some(state) => text_input(input_id(key), key.to_string(), state)
            .when(grow, |d| d.flex_grow(1.).min_w(px(0.)))
            .px(px(10.))
            .py(px(6.))
            .mono()
            .size_sm()
            .when(disabled, |d| d.opacity(0.5))
            .into_any_element(),
        None => div().h(px(34.)).into_any_element(),
    }
}

fn endpoint_errors(local: &EmbedLocal, path: &str, t: &Tokens, cx: &App) -> Option<AnyElement> {
    let errors: Vec<&FieldError> = local.errors.iter().filter(|e| e.path == path).collect();
    let blocking: Vec<FieldError> = errors.iter().map(|e| (*e).clone()).collect();
    error_list(path, &errors, &blocking, t, cx)
}

fn labelled_row(name: &str, key: &str, ctx: &Ctx, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(label(name).text_color(t.muted))
        .when_some(ctx.state.override_for(key), |d, variable| {
            d.child(override_text(key, variable, t))
        })
}

fn remote_endpoint(ctx: &Ctx, local: &EmbedLocal, cx: &App) -> AnyElement {
    let t = *tokens(cx);
    let state = ctx.state;
    let token_set = state.secret_set(TOKEN);
    let token_override = state.override_for(TOKEN);
    let model_draft = ctx
        .input(MODEL)
        .map(|i| i.read(cx).value().to_string())
        .unwrap_or_default();
    let model_changed = state
        .endpoint("model")
        .is_some_and(|saved| model_draft.trim() != saved);
    let url_block = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(labelled_row("Endpoint URL", URL, ctx, &t))
        .child(draft_input(ctx, URL, false))
        .children(endpoint_errors(local, URL, &t, cx));
    let model_block = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(labelled_row("Model (wire name)", MODEL, ctx, &t))
        .child(draft_input(ctx, MODEL, false))
        .when(model_changed, |d| {
            d.child(
                div()
                    .id("endpoint-model-warning")
                    .test_support()
                    .aria_label(WIRE_WARNING)
                    .size_xs()
                    .text_color(t.warn)
                    .child(wrapped_text(WIRE_WARNING)),
            )
        })
        .children(endpoint_errors(local, MODEL, &t, cx));
    let open = update(ctx, |s, cx| s.open_token(cx));
    let clear = update(ctx, |s, cx| s.clear_token(cx));
    let cancel = update(ctx, |s, cx| s.cancel_token(cx));
    let token_state = if token_set { "set" } else { "not set" };
    let token_row = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(label("Token").text_color(t.muted))
        .child(
            div()
                .id("token-state")
                .test_support()
                .aria_label(token_state)
                .child(
                    exact_text(token_state)
                        .semibold()
                        .text_size(px(11.))
                        .line_height(px(16.5))
                        .text_color(if token_set { t.accent } else { t.muted }),
                ),
        )
        .when_some(token_override.clone(), |d, variable| {
            d.child(override_text(TOKEN, variable, &t))
        })
        .child(div().flex_grow(1.))
        .when(!local.replacing && token_override.is_none(), |d| {
            d.child(
                button::secondary(
                    "token-replace",
                    if token_set { "Replace" } else { "Set token" },
                )
                .disabled(ctx.locked)
                .on_click(move |_, window, cx| open(window, cx)),
            )
        })
        .when(token_set && token_override.is_none(), |d| {
            d.child(
                button::danger("token-clear", "Clear")
                    .disabled(ctx.locked)
                    .on_click(move |_, window, cx| clear(window, cx)),
            )
        });
    let token_block = div()
        .id(super::nav::field_id(TOKEN))
        .relative()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(super::fields::bounds_recorder(ctx, TOKEN))
        .child(token_row)
        .when(local.replacing, |d| {
            d.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(draft_input(ctx, TOKEN, true))
                    .child(
                        div().flex().items_center().child(
                            text_button("token-cancel", "cancel", CANCEL_FACE, t.muted)
                                .on_click(move |_, window, cx| cancel(window, cx)),
                        ),
                    ),
            )
        })
        .child(
            div()
                .size_xs()
                .text_color(t.muted)
                .child(wrapped_text(WRITE_ONLY)),
        )
        .children(endpoint_errors(local, TOKEN, &t, cx));
    let save = update(ctx, |s, cx| s.save_endpoint(cx));
    let save_row = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            button::primary("endpoint-save", "Save endpoint")
                .disabled(ctx.locked)
                .on_click(move |_, window, cx| save(window, cx)),
        )
        .when_some(local.saved.clone(), |d, saved| {
            d.child(
                div()
                    .id("endpoint-saved")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(SharedString::from(saved.clone()))
                    .size_sm()
                    .text_color(t.accent)
                    .child(wrapped_text(saved)),
            )
        });
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(url_block)
        .child(model_block)
        .child(token_block)
        .child(save_row)
        .into_any_element()
}

fn test_connection(ctx: &Ctx, local: &EmbedLocal, cx: &App) -> AnyElement {
    let t = *tokens(cx);
    let unsaved = ctx.pending.paths().iter().any(|p| p.starts_with("embed."));
    let note = format!(
        "Embeds one sentence with the saved settings{}.",
        if unsaved {
            "; save first to test your unsaved changes"
        } else {
            ""
        }
    );
    let expected =
        super::pending::at(ctx.state.effective(), "embed.dimensions").and_then(Json::as_u64);
    let run = update(ctx, |s, cx| s.test_embedding(cx));
    let result = local.test.as_ref().map(|test| {
        let (text, mismatch) = result_text(test, expected);
        let (fill, colour) = if test.ok {
            (t.accent_soft, t.accent)
        } else {
            (t.warn_soft, t.warn)
        };
        let full = format!("{text}{}", mismatch.clone().unwrap_or_default());
        let mut segments = vec![Segment::text(text, SM_FACE)];
        if let Some(mismatch) = mismatch {
            segments.push(Segment::text(mismatch, SM_FACE).coloured(t.warn));
        }
        div()
            .id("embed-test-result")
            .test_support()
            .aria_label(SharedString::from(full))
            .relative()
            .px(px(12.))
            .py(px(8.))
            .size_sm()
            .text_color(colour)
            .child(css_box(fill, None, 6.))
            .child(inline_flow(SM_FACE, segments))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    button::secondary(
                        "embed-test",
                        if local.testing {
                            "Testing…"
                        } else {
                            "Test connection"
                        },
                    )
                    .disabled(local.testing)
                    .on_click(move |_, window, cx| run(window, cx)),
                )
                .child(
                    div()
                        .id("embed-test-note")
                        .test_support()
                        .aria_label(SharedString::from(note.clone()))
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .size_xs()
                        .text_color(t.muted)
                        .child(wrapped_text(note)),
                ),
        )
        .children(result)
        .into_any_element()
}

fn model_choice(ctx: &Ctx, cx: &App) -> AnyElement {
    let t = *tokens(cx);
    let choices = ctx.state.embed_models();
    let model = ctx.value("embed.model");
    let current = choices
        .iter()
        .position(|c| model.as_ref().is_some_and(|m| *m == c["model"]));
    let mut labels: Vec<SharedString> = Vec::new();
    let mut ids: Vec<Option<String>> = Vec::new();
    if current.is_none() {
        let shown = match &model {
            Some(Json::String(s)) => s.clone(),
            None | Some(Json::Null) => String::new(),
            Some(other) => other.to_string(),
        };
        labels.push(format!("Custom: {shown}").into());
        ids.push(None);
    }
    for choice in &choices {
        labels.push(
            format!(
                "{} ({})",
                choice["label"].as_str().unwrap_or_default(),
                choice["size"].as_str().unwrap_or_default()
            )
            .into(),
        );
        ids.push(choice["id"].as_str().map(str::to_string));
    }
    let selected = current.unwrap_or(0);
    let view = ctx.view.clone();
    let summary = current.map_or(CUSTOM_MODEL.to_string(), |ix| choice_summary(&choices[ix]));
    div()
        .id("embed-choice")
        .test_support()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(label("Model").text_color(t.muted))
        .child(
            select(
                "select-embed.choice",
                labels,
                selected,
                SelectSize::Sm,
                move |ix, _, cx| {
                    if let Some(Some(id)) = ids.get(ix) {
                        let id = id.clone();
                        let _ = view.update(cx, |s, cx| s.choose_model(&id, cx));
                    }
                },
            )
            .disabled(ctx.locked)
            .pad_x(INPUT_PAD_X)
            .w(px(320.)),
        )
        .child(
            div()
                .id("embed-choice-summary")
                .test_support()
                .aria_label(SharedString::from(summary.clone()))
                .size_xs()
                .text_color(t.muted)
                .child(wrapped_text(summary)),
        )
        .into_any_element()
}

fn reindex_warning(t: &Tokens) -> AnyElement {
    div()
        .id("reindex-warning")
        .test_support()
        .role(Role::Alert)
        .aria_label(REINDEX_WARNING)
        .relative()
        .px(px(12.))
        .py(px(10.))
        .size_sm()
        .text_color(t.warn)
        .child(css_box(t.warn_soft, None, 6.))
        .child(inline_flow(
            SM_FACE,
            vec![
                Segment::text(
                    "Changing the embedding model or its dimensions makes the existing index unreadable: every document has to be embedded again with ",
                    SM_FACE,
                ),
                Segment::chip(
                    "rhebok index --reindex",
                    CODE_FACE,
                    Some(Chip {
                        background: t.sunken,
                        pad: 4.,
                        radius: 4.,
                    }),
                ),
                Segment::text(". You can start that from here after saving.", SM_FACE),
            ],
        ))
        .into_any_element()
}

pub fn embedding_section(ctx: &Ctx, local: &EmbedLocal, cx: &App) -> Vec<AnyElement> {
    let t = *tokens(cx);
    let state = ctx.state;
    let mode = local.mode(state);
    let backend = Mode::of(state);
    let view = ctx.view.clone();
    let source = segmented(
        "segmented-embed-source",
        &["Local Ollama", "Remote endpoint"],
        if mode == Mode::Ollama { 0 } else { 1 },
        move |ix, _, cx| {
            let mode = if ix == 0 { Mode::Ollama } else { Mode::Remote };
            let _ = view.update(cx, |s, cx| s.set_embed_mode(mode, cx));
        },
    )
    .disabled(ctx.locked);
    let switch = update(ctx, |s, cx| s.use_local_ollama(cx));
    let source_card = card("card-embed-source", "Where embeddings come from", None, cx)
        .child(div().flex().child(source))
        .when_some(state.endpoint("error"), |d, error| {
            let text = format!("The env file has a problem: {error}");
            d.child(
                div()
                    .id("endpoint-error")
                    .test_support()
                    .aria_label(SharedString::from(text.clone()))
                    .size_sm()
                    .text_color(t.warn)
                    .child(wrapped_text(text)),
            )
        })
        .when(mode == Mode::Ollama && backend == Mode::Remote, |d| {
            d.child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(10.))
                    .size_sm()
                    .child(css_box(t.sunken, None, 6.))
                    .child(
                        div()
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .child(wrapped_text(SWITCH_NOTE)),
                    )
                    .child(
                        button::secondary("use-local-ollama", "Use local Ollama")
                            .disabled(
                                ctx.locked || local.switching || state.override_for(URL).is_some(),
                            )
                            .on_click(move |_, window, cx| switch(window, cx)),
                    ),
            )
        })
        .when(mode == Mode::Remote, |d| {
            d.child(remote_endpoint(ctx, local, cx))
        })
        .child(test_connection(ctx, local, cx));
    let model_card = card("card-model", "Model", None, cx)
        .when(ctx.pending.needs_reindex(), |d| {
            d.child(reindex_warning(&t))
        })
        .when(mode == Mode::Ollama, |d| {
            d.child(model_choice(ctx, cx)).child(text_field(
                ctx,
                Field::new("embed.ollama_url", "Ollama URL"),
                true,
                cx,
            ))
        })
        .child(text_field(
            ctx,
            match mode {
                Mode::Remote => {
                    Field::new("embed.model", "Embedding model").hint(REMOTE_MODEL_HINT)
                }
                Mode::Ollama => Field::new("embed.model", "Embedding model"),
            },
            true,
            cx,
        ))
        .child(number_field(
            ctx,
            "embed.dimensions",
            "Dimensions",
            None,
            cx,
        ))
        .child(select_field(
            ctx,
            Field::new("embed.prefix_scheme", "Prefix scheme"),
            &PREFIX_SCHEMES,
            Some("detect from the model"),
            cx,
        ));
    let mut throughput = Vec::new();
    if mode == Mode::Ollama {
        throughput.push(shell(
            ctx,
            Field::new("embed.keep_alive", "Keep alive").hint(Hint::from(KEEP_ALIVE_HINT)),
            number_input(ctx, "embed.keep_alive", 140., None),
            cx,
        ));
    }
    throughput.push(number_field(
        ctx,
        "embed.concurrency",
        "Concurrency",
        None,
        cx,
    ));
    throughput.push(number_field(ctx, "embed.batch", "Batch size", None, cx));
    throughput.push(number_field(
        ctx,
        "embed.chunk_tokens",
        "Chunk tokens",
        None,
        cx,
    ));
    let throughput_card = card("card-throughput", "Throughput", None, cx).child(grid(throughput));
    let enrichment = card("card-enrichment", "Contextual enrichment", None, cx)
        .child(toggle_field(
            ctx,
            "embed.contextual",
            "Enrich chunks with context",
            None,
            cx,
        ))
        .child(text_field(
            ctx,
            Field::new("embed.enrich_model", "Enrichment model"),
            true,
            cx,
        ));
    vec![
        source_card.into_any_element(),
        model_card.into_any_element(),
        throughput_card.into_any_element(),
        enrichment.into_any_element(),
    ]
}

#[cfg(test)]
mod tests {
    use rhebok::dashboard::core::embed::EmbedTest;
    use serde_json::json;

    use super::{choice_summary, result_text};

    fn ok(dimensions: usize, model: Option<&str>) -> EmbedTest {
        EmbedTest {
            ok: true,
            latency_ms: 5,
            dimensions: Some(dimensions),
            backend: "ollama",
            url: None,
            model: model.map(str::to_string),
            error: None,
        }
    }

    #[test]
    fn a_result_names_a_dimension_mismatch_apart() {
        assert_eq!(
            result_text(&ok(1024, Some("m")), Some(1024)),
            (
                "Connected to Ollama in 5 ms · 1024 dimensions · m".to_string(),
                None
            )
        );
        assert_eq!(
            result_text(&ok(768, None), Some(1024)).1.as_deref(),
            Some(" · the index expects 1024 dimensions, so this model needs a full re-index")
        );
        let mut failed = ok(1, None);
        failed.ok = false;
        failed.dimensions = None;
        failed.error = None;
        assert_eq!(
            result_text(&failed, None).0,
            "Could not embed through ollama: unknown error"
        );
    }

    #[test]
    fn a_choice_summary_prints_its_gates_as_javascript_numbers() {
        let choice = json!({
            "summary": "Small", "hook_threshold": 0.66, "mcp_threshold": 0.55, "min_ollama": null
        });
        assert_eq!(
            choice_summary(&choice),
            "Small. Choosing a model also sets its dimensions and both relevance gates (hook 0.66, MCP 0.55), because each model scores on its own scale."
        );
    }
}
