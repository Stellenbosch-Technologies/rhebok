use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::Root;
use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{point, px, AppContext, Entity, SharedString, TestAppContext, WindowHandle};
use rhebok::bench::BenchTier;
use rhebok::config::check::FieldError;
use rhebok::dashboard::core::config::{set_embed_endpoint, ConfigView, EmbedEndpoint};
use rhebok::dashboard::core::embed::EmbedTest;
use rhebok::dashboard::core::DashboardError;
use serde_json::json;
use toml::Value;

use super::fields::parse_keep_alive;
use super::nav::Section;
use super::tests::{
    click, fixture, label, open, render, shown, stats, type_into, wait, Folder, CHECK_DEBOUNCE,
};
use super::Settings;
use crate::backend::FixtureBackend;

fn section(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    settings: &Entity<Settings>,
    section: Section,
) {
    settings.update(cx, |s, cx| s.pick_section(section, cx));
    render(cx, window);
}

fn input_text(cx: &mut TestAppContext, settings: &Entity<Settings>, key: &str) -> Option<String> {
    settings.read_with(cx, |s, cx| {
        s.input(key).map(|i| i.read(cx).value().to_string())
    })
}

fn pending(cx: &mut TestAppContext, settings: &Entity<Settings>, path: &str) -> Option<Value> {
    settings.read_with(cx, |s, _| s.pending().get(path).and_then(|v| v.cloned()))
}

fn slider_value(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> String {
    let id = gpui_kit::SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.find(id).value().unwrap_or_default().to_string()
    })
    .unwrap()
}

fn tier(name: &str, recall: f32, p50: u64) -> BenchTier {
    BenchTier {
        tier: name.to_string(),
        recall_at_5: recall,
        p50_ms: p50,
        p95_ms: p50 * 2,
        absent: Vec::new(),
    }
}

#[gpui_kit::test]
fn each_tier_option_shows_its_bench_recall_and_latency(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let mut with_bench = stats();
    with_bench.bench = Some(vec![
        tier("instant", 0.7, 54),
        tier("fast", 0.125, 101),
        tier("balanced", 0.92, 204),
        tier("thorough", 0.92, 231),
        tier("exhaustive", 0.93, 338),
    ]);
    fixture.stats.set(Ok(with_bench));
    let (window, shell, settings) = open(cx, fixture.clone(), 900.);
    shell.update(cx, |s, cx| s.refresh_stats(cx));
    section(cx, window, &settings, Section::Retrieval);

    assert_eq!(
        label(cx, window, "segmented-hook.quality-1").as_deref(),
        Some("fast")
    );
    assert_eq!(
        label(cx, window, "segmented-hook.quality-1-bench").as_deref(),
        Some("R@5 0.13 · 101ms"),
        "recall is toFixed(2), which rounds 0.125 half up"
    );
    assert_eq!(
        label(cx, window, "segmented-mcp.quality-4-bench").as_deref(),
        Some("R@5 0.93 · 338ms")
    );
    assert!(!shown(cx, window, "hook.quality-bench-hint"));

    let tall = super::tests::bounds(cx, window, "segmented-hook.quality-1");
    assert_eq!(
        f32::from(tall.size.height),
        39.5,
        "two leading-tight lines (15 + 12.5) inside py-1.5"
    );

    fixture.stats.set(Ok(stats()));
    shell.update(cx, |s, cx| s.refresh_stats(cx));
    render(cx, window);
    assert!(!shown(cx, window, "segmented-hook.quality-1-bench"));
    assert_eq!(
        label(cx, window, "hook.quality-bench-hint").as_deref(),
        Some("Run rhebok bench to see each tier's measured recall and latency here.")
    );
    let short = super::tests::bounds(cx, window, "segmented-hook.quality-1");
    assert_eq!(f32::from(short.size.height), 27., "one leading-tight line");
}

#[gpui_kit::test]
fn the_threshold_slider_and_number_stay_in_step(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture, 900.);
    section(cx, window, &settings, Section::Retrieval);
    assert_eq!(slider_value(cx, window, "slider-hook.threshold"), "0.66");

    let track = super::tests::bounds(cx, window, "slider-hook.threshold");
    let travel = f32::from(track.size.width) - 2. - 15.;
    let at = |v: f32| point(px(1. + 7.5 + v * travel), track.size.height / 2.);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click_at("slider-hook.threshold", at(0.3), cx);
    })
    .unwrap();
    render(cx, window);
    assert_eq!(
        pending(cx, &settings, "hook.threshold"),
        Some(Value::Float(0.3))
    );
    assert_eq!(
        input_text(cx, &settings, "hook.threshold").as_deref(),
        Some("0.3")
    );
    assert_eq!(slider_value(cx, window, "slider-hook.threshold"), "0.3");

    let from = track.origin + at(0.3);
    let to = track.origin + at(0.774);
    cx.update_window(window.into(), |_, window, cx| window.drag(from, to, cx))
        .unwrap();
    render(cx, window);
    assert_eq!(
        pending(cx, &settings, "hook.threshold"),
        Some(Value::Float(0.77)),
        "a drag moves in steps of 0.01"
    );
    assert_eq!(
        input_text(cx, &settings, "hook.threshold").as_deref(),
        Some("0.77")
    );

    type_into(cx, window, "input-hook.threshold", "5");
    assert_eq!(
        pending(cx, &settings, "hook.threshold"),
        Some(Value::Float(0.775))
    );
    assert_eq!(
        slider_value(cx, window, "slider-hook.threshold"),
        "0.78",
        "the slider shows the number rounded to its step"
    );
    let number = super::tests::bounds(cx, window, "input-hook.threshold");
    assert_eq!(f32::from(number.size.width), 90.);
}

#[gpui_kit::test]
fn the_token_is_never_rendered_back(cx: &mut TestAppContext) {
    let secret = "sk-native-never-shown";
    let folder = Folder::new();
    let before = folder.view("index_transcripts = false\n");
    let after = set_embed_endpoint(
        &folder.config(),
        EmbedEndpoint {
            url: Some(Some("http://127.0.0.1:9/v1".into())),
            model: Some(Some("stub-model".into())),
            token: Some(Some(secret.into())),
        },
    )
    .unwrap();
    assert!(!after.0.to_string().contains(secret));
    let fixture = fixture(before);
    fixture.set_embed_endpoint.set(Ok(after));
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    section(cx, window, &settings, Section::Embedding);

    click(cx, window, "segmented-embed-source-1");
    assert_eq!(label(cx, window, "token-state").as_deref(), Some("not set"));
    type_into(cx, window, "input-endpoint.url", "http://127.0.0.1:9/v1");
    type_into(cx, window, "input-endpoint.model", "stub-model");
    assert_eq!(
        label(cx, window, "token-replace").as_deref(),
        Some("Set token")
    );
    click(cx, window, "token-replace");
    type_into(cx, window, "input-embed.token", secret);
    assert!(settings.read_with(cx, |s, cx| s
        .input("embed.token")
        .is_some_and(|i| i.read(cx).presentation().is_masked())));
    click(cx, window, "endpoint-save");

    let sent: Vec<String> = fixture
        .requests()
        .into_iter()
        .filter(|r| r.starts_with("set_embed_endpoint"))
        .collect();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains(secret), "{}", sent[0]);
    assert!(sent[0].contains("stub-model"), "{}", sent[0]);
    assert_eq!(label(cx, window, "token-state").as_deref(), Some("set"));
    assert_eq!(
        label(cx, window, "token-replace").as_deref(),
        Some("Replace")
    );
    assert!(shown(cx, window, "token-clear"));
    assert!(!shown(cx, window, "input-embed.token"), "the field closes");
    assert_eq!(
        label(cx, window, "endpoint-saved").as_deref(),
        Some("Endpoint saved to the env file beside config.toml.")
    );
    assert!(
        settings.read_with(cx, |s, _| s.pending().is_empty()),
        "an endpoint save is not a pending config edit"
    );

    let leaks = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window
                .elements()
                .into_iter()
                .filter(|e| {
                    [e.label(), e.value(), e.description(), e.placeholder()]
                        .into_iter()
                        .flatten()
                        .any(|text| text.contains(secret))
                })
                .map(|e| format!("{:?}", e.path()))
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert!(leaks.is_empty(), "elements holding the token: {leaks:?}");
    settings.read_with(cx, |s, cx| {
        assert!(
            s.inputs_hold(secret, cx).is_empty(),
            "no input still holds the token"
        );
    });

    click(cx, window, "token-replace");
    assert_eq!(
        input_text(cx, &settings, "embed.token").as_deref(),
        Some(""),
        "replacing starts from an empty field"
    );
}

#[gpui_kit::test]
fn test_connection_reports_dimensions_or_the_error(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    fixture.embed_test.set(EmbedTest {
        ok: true,
        latency_ms: 12,
        dimensions: Some(512),
        backend: "ollama",
        url: Some("http://127.0.0.1:11434".into()),
        model: Some("qwen3-embedding:0.6b".into()),
        error: None,
    });
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    section(cx, window, &settings, Section::Embedding);
    assert_eq!(
        label(cx, window, "embed-test-note").as_deref(),
        Some("Embeds one sentence with the saved settings.")
    );
    settings.update(cx, |s, cx| s.test_embedding(cx));
    assert_eq!(label(cx, window, "embed-test").as_deref(), Some("Testing…"));
    wait(cx, window, Duration::from_millis(1));
    assert_eq!(
        label(cx, window, "embed-test-result").as_deref(),
        Some("Connected to Ollama in 12 ms · 512 dimensions · qwen3-embedding:0.6b")
    );
    assert_eq!(
        label(cx, window, "embed-test").as_deref(),
        Some("Test connection")
    );

    fixture.embed_test.set(EmbedTest {
        ok: true,
        latency_ms: 30,
        dimensions: Some(768),
        backend: "remote",
        url: Some("http://127.0.0.1:9/v1".into()),
        model: None,
        error: None,
    });
    click(cx, window, "embed-test");
    wait(cx, window, Duration::from_millis(1));
    assert_eq!(
        label(cx, window, "embed-test-result").as_deref(),
        Some("Connected to the remote endpoint in 30 ms · 768 dimensions · the index expects 512 dimensions, so this model needs a full re-index")
    );

    fixture.embed_test.set(EmbedTest {
        ok: false,
        latency_ms: 3,
        dimensions: None,
        backend: "remote",
        url: Some("http://127.0.0.1:9/v1".into()),
        model: Some("stub-model".into()),
        error: Some("connection refused".into()),
    });
    click(cx, window, "embed-test");
    wait(cx, window, Duration::from_millis(1));
    assert_eq!(
        label(cx, window, "embed-test-result").as_deref(),
        Some("Could not embed through http://127.0.0.1:9/v1: connection refused")
    );

    settings.update(cx, |s, cx| {
        s.set("embed.batch", Some(Value::Integer(8)), cx)
    });
    render(cx, window);
    assert_eq!(
        label(cx, window, "embed-test-note").as_deref(),
        Some(
            "Embeds one sentence with the saved settings; save first to test your unsaved changes."
        )
    );
}

#[gpui_kit::test]
fn changing_the_model_warns_that_a_full_reindex_is_needed(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture, 900.);
    section(cx, window, &settings, Section::Embedding);
    assert!(!shown(cx, window, "reindex-warning"));
    type_into(cx, window, "input-embed.model", "-x");
    assert_eq!(
        label(cx, window, "reindex-warning").as_deref(),
        Some("Changing the embedding model or its dimensions makes the existing index unreadable: every document has to be embedded again with rhebok index --reindex. You can start that from here after saving.")
    );
    wait(cx, window, CHECK_DEBOUNCE);
    click(cx, window, "save-bar-discard");
    assert!(!shown(cx, window, "reindex-warning"));

    click(cx, window, "select-embed.choice");
    click(cx, window, "select-embed.choice-option-1");
    for (path, value) in [
        (
            "embed.model",
            Value::String("embeddinggemma-2:270m-bf16-text".into()),
        ),
        ("embed.dimensions", Value::Integer(512)),
        ("hook.threshold", Value::Float(0.82)),
        ("mcp.threshold", Value::Float(0.68)),
    ] {
        assert_eq!(pending(cx, &settings, path), Some(value), "{path}");
    }
    assert!(shown(cx, window, "reindex-warning"));
    assert_eq!(
        input_text(cx, &settings, "embed.model").as_deref(),
        Some("embeddinggemma-2:270m-bf16-text"),
        "the model field follows the picker"
    );
    let summary = label(cx, window, "embed-choice-summary").unwrap_or_default();
    assert!(
        summary.contains("(hook 0.82, MCP 0.68)")
            && summary.contains("It needs Ollama 0.40.0 or newer"),
        "{summary}"
    );
}

#[gpui_kit::test]
fn an_endpoint_refusal_shows_its_errors_and_keeps_the_drafts(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    fixture
        .set_embed_endpoint
        .set(Err(DashboardError::Unprocessable(vec![FieldError::new(
            "endpoint.model",
            "is required while an endpoint URL is set",
        )])));
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    section(cx, window, &settings, Section::Embedding);
    click(cx, window, "segmented-embed-source-1");
    type_into(cx, window, "input-endpoint.url", "http://127.0.0.1:9/v1");
    settings.update(cx, |s, cx| {
        s.save_endpoint(cx);
        s.set("embed.batch", Some(Value::Integer(8)), cx);
    });
    assert!(
        settings.read_with(cx, |s, _| s.pending().is_empty()),
        "the form is locked while the endpoint is being written"
    );
    wait(cx, window, Duration::from_millis(1));
    let sent: Vec<String> = fixture
        .requests()
        .into_iter()
        .filter(|r| r.starts_with("set_embed_endpoint"))
        .collect();
    assert_eq!(
        sent,
        ["set_embed_endpoint EmbedEndpoint { url: Some(Some(\"http://127.0.0.1:9/v1\")), model: None, token: None }"],
        "only what changed is sent"
    );
    assert_eq!(
        label(cx, window, "field-endpoint-model-error-0").as_deref(),
        Some("is required while an endpoint URL is set")
    );
    assert!(!shown(cx, window, "endpoint-saved"));
    assert_eq!(
        input_text(cx, &settings, "endpoint.url").as_deref(),
        Some("http://127.0.0.1:9/v1")
    );

    section(cx, window, &settings, Section::Memory);
    section(cx, window, &settings, Section::Embedding);
    assert_eq!(
        label(cx, window, "segmented-embed-source-0").map(|_| ()),
        Some(())
    );
    assert!(
        !shown(cx, window, "input-endpoint.url"),
        "leaving the section drops the unsaved mode, as remounting does on the web"
    );
}

#[gpui_kit::test]
fn a_remote_endpoint_overridden_by_the_environment_cannot_be_edited(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let mut view = folder.view("index_transcripts = false\n");
    view.0["endpoint"] = json!({
        "backend": "remote", "url": "https://env.example/v1", "model": "env-model", "error": null
    });
    view.0["env_overrides"] = json!([
        { "variable": "RHEBOK_EMBED_URL", "setting": "endpoint.url" },
        { "variable": "RHEBOK_EMBED_TOKEN", "setting": "embed.token" }
    ]);
    view.0["secrets"]["embed.token"] = json!("set");
    let fixture = fixture(ConfigView(view.0));
    let (window, _, settings) = open(cx, fixture, 900.);
    section(cx, window, &settings, Section::Embedding);
    assert_eq!(
        input_text(cx, &settings, "endpoint.url").as_deref(),
        Some("https://env.example/v1")
    );
    assert!(settings.read_with(cx, |s, cx| s
        .input("endpoint.url")
        .is_some_and(|i| i.read(cx).presentation().is_disabled())));
    assert!(settings.read_with(cx, |s, cx| s
        .input("endpoint.model")
        .is_some_and(|i| !i.read(cx).presentation().is_disabled())));
    assert_eq!(
        label(cx, window, "endpoint.url-override").as_deref(),
        Some("overridden by $RHEBOK_EMBED_URL")
    );
    assert!(!shown(cx, window, "token-replace"));
    assert!(!shown(cx, window, "token-clear"));
    assert!(!shown(cx, window, "use-local-ollama"));
}

#[gpui_kit::test]
fn use_local_ollama_removes_only_the_url(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let mut view = folder.view("index_transcripts = false\n");
    view.0["endpoint"] = json!({
        "backend": "remote", "url": "https://remote.example/v1", "model": "m", "error": null
    });
    let after = folder.view("index_transcripts = false\n");
    let fixture = fixture(ConfigView(view.0));
    fixture.set_embed_endpoint.set(Ok(after));
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    section(cx, window, &settings, Section::Embedding);
    assert!(shown(cx, window, "input-endpoint.url"));
    click(cx, window, "segmented-embed-source-0");
    assert!(shown(cx, window, "use-local-ollama"));
    click(cx, window, "use-local-ollama");
    assert!(fixture.requests().contains(
        &"set_embed_endpoint EmbedEndpoint { url: Some(None), model: None, token: None }"
            .to_string()
    ));
    assert!(!shown(cx, window, "use-local-ollama"));
    assert!(!shown(cx, window, "input-endpoint.url"));
}

#[gpui_kit::test]
fn keep_alive_reads_digits_as_a_number_and_anything_else_as_a_duration(cx: &mut TestAppContext) {
    assert_eq!(parse_keep_alive("30"), Value::Integer(30));
    assert_eq!(parse_keep_alive(" -1 "), Value::Integer(-1));
    assert_eq!(parse_keep_alive("30m"), Value::String("30m".into()));
    assert_eq!(parse_keep_alive("1.5"), Value::String("1.5".into()));
    assert_eq!(parse_keep_alive(""), Value::String(String::new()));

    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n[embed]\nkeep_alive = 5\n"));
    let (window, _, settings) = open(cx, fixture, 1800.);
    section(cx, window, &settings, Section::Embedding);
    assert_eq!(
        input_text(cx, &settings, "embed.keep_alive").as_deref(),
        Some("5")
    );
    let retype = |cx: &mut TestAppContext, text: &str| {
        click(cx, window, "input-embed.keep_alive");
        cx.update_window(window.into(), |_, window, cx| {
            window.press("cmd-a", cx);
            window.input(text, cx);
        })
        .unwrap();
        cx.run_until_parked();
        render(cx, window);
    };
    retype(cx, "45");
    assert_eq!(
        pending(cx, &settings, "embed.keep_alive"),
        Some(Value::Integer(45))
    );
    retype(cx, "45m");
    assert_eq!(
        pending(cx, &settings, "embed.keep_alive"),
        Some(Value::String("45m".into()))
    );
    assert_eq!(
        input_text(cx, &settings, "embed.keep_alive").as_deref(),
        Some("45m")
    );
}

#[gpui_kit::test]
fn backup_targets_toggle_into_the_list(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture, 900.);
    section(cx, window, &settings, Section::Updates);
    click(cx, window, "backup-target-drive");
    click(cx, window, "backup-target-s3");
    assert_eq!(
        pending(cx, &settings, "backup.targets"),
        Some(Value::Array(vec![
            Value::String("drive".into()),
            Value::String("s3".into())
        ]))
    );
    click(cx, window, "backup-target-drive");
    assert_eq!(
        pending(cx, &settings, "backup.targets"),
        Some(Value::Array(vec![Value::String("s3".into())]))
    );
    assert_eq!(
        label(cx, window, "secret-backup.key").as_deref(),
        Some("not present")
    );
}

#[gpui_kit::test]
fn every_section_renders_its_cards(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture: Arc<FixtureBackend> = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture, 900.);
    for (s, cards) in [
        (
            Section::Retrieval,
            &["card-hook", "card-mcp", "card-claude-code"][..],
        ),
        (
            Section::Embedding,
            &[
                "card-embed-source",
                "card-model",
                "card-throughput",
                "card-enrichment",
            ][..],
        ),
        (Section::Memory, &["card-memories", "card-distillation"][..]),
        (Section::Documents, &["card-pdf"][..]),
        (
            Section::Updates,
            &["card-updates", "card-backup", "card-s3", "card-drive"][..],
        ),
    ] {
        section(cx, window, &settings, s);
        for card in cards {
            assert!(shown(cx, window, card), "{s:?} shows {card}");
        }
    }
}

#[gpui_kit::test]
fn only_the_endpoint_inputs_follow_an_environment_override(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let mut view = folder.view("index_transcripts = false\n[embed]\nmodel = \"m\"\n");
    view.0["env_overrides"] =
        json!([{ "variable": "RHEBOK_TEST_MODEL", "setting": "embed.model" }]);
    let fixture = fixture(ConfigView(view.0));
    let (window, _, settings) = open(cx, fixture, 900.);
    section(cx, window, &settings, Section::Embedding);
    assert!(
        settings.read_with(cx, |s, cx| s
            .input("embed.model")
            .is_some_and(|i| !i.read(cx).presentation().is_disabled())),
        "the web never passes an override to a TextField"
    );
    assert!(shown(cx, window, "field-embed-model-reset"));
}

fn key(cx: &mut TestAppContext, window: WindowHandle<Root>, keys: &str) {
    cx.update_window(window.into(), |_, window, cx| window.press(keys, cx))
        .unwrap();
    cx.run_until_parked();
    render(cx, window);
}

#[gpui_kit::test]
fn the_threshold_slider_moves_on_the_keys_a_range_input_takes(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture, 900.);
    section(cx, window, &settings, Section::Retrieval);
    let track = super::tests::bounds(cx, window, "slider-hook.threshold");
    let travel = f32::from(track.size.width) - 2. - 15.;
    let thumb = point(px(1. + 7.5 + 0.66 * travel), track.size.height / 2.);
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click_at("slider-hook.threshold", thumb, cx);
    })
    .unwrap();
    render(cx, window);
    assert_eq!(
        pending(cx, &settings, "hook.threshold"),
        None,
        "a press on the thumb moves nothing"
    );
    for (keys, expected) in [
        ("right", "0.67"),
        ("up", "0.68"),
        ("left", "0.67"),
        ("down", "0.66"),
        ("pageup", "0.76"),
        ("pagedown", "0.66"),
        ("end", "1"),
        ("home", "0"),
    ] {
        key(cx, window, keys);
        assert_eq!(
            slider_value(cx, window, "slider-hook.threshold"),
            expected,
            "{keys}"
        );
    }
    assert_eq!(
        pending(cx, &settings, "hook.threshold"),
        Some(Value::Integer(0))
    );
    let slider = super::tests::bounds(cx, window, "slider-hook.threshold");
    let rings = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let accent = crate::theme::tokens(cx).accent;
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter(|q| q.border_color == accent && q.border_widths.top.0 > 0.)
                .map(|q| {
                    (
                        q.bounds.origin.x.0 / scale,
                        q.bounds.origin.y.0 / scale,
                        q.bounds.size.width.0 / scale,
                        q.bounds.size.height.0 / scale,
                        q.border_widths.top.0 / scale,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap();
    let outset = (
        f32::from(slider.left()).round() - 2.,
        f32::from(slider.top()).round() - 2.,
        f32::from(slider.size.width).round() + 4.,
        f32::from(slider.size.height).round() + 4.,
        2.,
    );
    let mut rings = rings;
    rings.dedup();
    assert_eq!(
        rings,
        [outset],
        "a keyboard-focused range input has Chrome's 2px accent outline at offset 0"
    );
}

#[gpui_kit::test]
fn a_late_endpoint_answer_does_not_reach_a_section_left_meanwhile(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    fixture
        .set_embed_endpoint
        .set(Err(DashboardError::Unprocessable(vec![FieldError::new(
            "endpoint.model",
            "is required while an endpoint URL is set",
        )])));
    let (window, _, settings) = open(cx, fixture, 900.);
    section(cx, window, &settings, Section::Embedding);
    click(cx, window, "segmented-embed-source-1");
    type_into(cx, window, "input-endpoint.url", "http://127.0.0.1:9/v1");
    settings.update(cx, |s, cx| {
        s.save_endpoint(cx);
        s.pick_section(Section::Memory, cx);
    });
    wait(cx, window, Duration::from_millis(1));
    assert!(
        settings.read_with(cx, |s, _| s.embed_local().errors.is_empty()),
        "a late refusal is dropped with the form it belonged to"
    );
    section(cx, window, &settings, Section::Embedding);
    click(cx, window, "segmented-embed-source-1");
    assert!(
        !shown(cx, window, "field-endpoint-model-error-0"),
        "the refusal belonged to the form that was left"
    );
    assert!(settings.read_with(cx, |s, _| s.embed_local().errors.is_empty()));
}

#[gpui_kit::test]
fn save_and_discard_wait_while_the_endpoint_is_written(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    fixture
        .config_save
        .set(Ok(folder.view("index_transcripts = true\n")));
    let (window, _, settings) = open(cx, fixture.clone(), 900.);
    section(cx, window, &settings, Section::Embedding);
    settings.update(cx, |s, cx| {
        s.set("embed.batch", Some(Value::Integer(8)), cx)
    });
    wait(cx, window, CHECK_DEBOUNCE);
    click(cx, window, "segmented-embed-source-1");
    settings.update(cx, |s, cx| {
        s.save_endpoint(cx);
        s.discard(cx);
        s.save(cx);
    });
    assert_eq!(
        settings.read_with(cx, |s, _| s.pending().len()),
        1,
        "discard waits for the endpoint write"
    );
    wait(cx, window, Duration::from_millis(1));
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|r| r.starts_with("config_save")),
        "save waits for the endpoint write"
    );
}

#[gpui_kit::test]
fn a_fractional_quality_selects_no_tier(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(
        folder.view("index_transcripts = false\n[hook]\nquality = 1.5\n[mcp]\nquality = 2.0\n"),
    );
    let (window, _, settings) = open(cx, fixture, 900.);
    section(cx, window, &settings, Section::Retrieval);
    let chosen = |cx: &mut TestAppContext, prefix: &str| -> Vec<usize> {
        (0..5)
            .filter(|ix| {
                let id = gpui_kit::SharedString::from(format!("{prefix}-{ix}"));
                cx.update_window(window.into(), |_, window, cx| {
                    window.render_frame(cx);
                    window.find(id).checked() == Some(true)
                })
                .unwrap()
            })
            .collect()
    };
    assert_eq!(chosen(cx, "segmented-hook.quality"), Vec::<usize>::new());
    assert_eq!(chosen(cx, "segmented-mcp.quality"), [2]);
}

#[gpui_kit::test]
fn a_settings_select_starts_its_text_inside_px_2_5(_cx: &mut TestAppContext) {
    let mut cx = TestAppContext::build_with_text_system(
        gpui_kit::TestDispatcher::new(0),
        None,
        Arc::new(gpui_apple::AppleTextSystem::new()),
    );
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let (window, _, settings) = open(&mut cx, fixture, 900.);
    section(&mut cx, window, &settings, Section::Documents);
    let frame = super::tests::bounds(&mut cx, window, "select-pdf.ocr");
    let text = super::tests::bounds(&mut cx, window, "select-pdf.ocr-text");
    assert_eq!(
        f32::from(text.left() - frame.left()),
        15.,
        "1px border, px-2.5 and Chrome's 4px menulist inset"
    );
}
struct PlaceholderProbe(Entity<gpui_kit::base::input::InputState>);

impl gpui_kit::Render for PlaceholderProbe {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        _: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        gpui_kit::div()
    }
}

#[gpui_kit::test]
fn a_placeholder_follows_the_value_in_use(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        crate::theme::install(cx);
        cx.open_window(Default::default(), |window, cx| {
            let t = *crate::theme::tokens(cx);
            cx.new(|cx| {
                PlaceholderProbe(crate::widgets::text_input::input_state(
                    "0.5", "", &t, window, cx,
                ))
            })
        })
        .unwrap()
    });
    let input = window.read_with(cx, |probe, _| probe.0.clone()).unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        super::fields::sync_placeholder(&input, "0.7", window, cx)
    })
    .unwrap();
    let shown = input.read_with(cx, |state, _| {
        state.presentation().placeholder().to_string()
    });
    assert_eq!(shown, "0.7");
}

#[gpui_kit::test]
fn the_bench_line_is_set_in_mono(_cx: &mut TestAppContext) {
    let mut cx = TestAppContext::build_with_text_system(
        gpui_kit::TestDispatcher::new(0),
        None,
        Arc::new(gpui_apple::AppleTextSystem::new()),
    );
    let folder = Folder::new();
    let fixture = fixture(folder.view("index_transcripts = false\n"));
    let mut with_bench = stats();
    with_bench.bench = Some(vec![tier("fast", 0.125, 101)]);
    fixture.stats.set(Ok(with_bench));
    let (window, shell, settings) = open(&mut cx, fixture, 900.);
    shell.update(&mut cx, |s, cx| s.refresh_stats(cx));
    section(&mut cx, window, &settings, Section::Retrieval);
    let sub = super::tests::bounds(&mut cx, window, "segmented-hook.quality-1-bench");
    assert_eq!(
        f32::from(sub.size.width),
        96.,
        "sixteen IBM Plex Mono advances of 6px at 10px"
    );
}

const RAW_HEIGHT: f32 = 2400.;

fn raw_value(cx: &mut TestAppContext, settings: &Entity<Settings>) -> String {
    settings.read_with(cx, |s, cx| {
        s.raw_area()
            .map(|a| a.read(cx).value().to_string())
            .expect("the raw editor exists")
    })
}

fn replace_raw(cx: &mut TestAppContext, window: WindowHandle<Root>, lines: &[&str]) {
    click(cx, window, super::raw_toml::AREA_ID);
    cx.update_window(window.into(), |_, window, cx| {
        window.press("cmd-a", cx);
        for (ix, line) in lines.iter().enumerate() {
            if ix > 0 {
                window.press("enter", cx);
            }
            window.input(line, cx);
        }
    })
    .unwrap();
    cx.run_until_parked();
    render(cx, window);
}

fn raw_file(folder: &Folder, body: &str) -> ConfigView {
    folder.view(body)
}

#[gpui_kit::test]
fn apply_to_form_turns_a_raw_edit_into_pending_rows(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let view = raw_file(
        &folder,
        "# keep me\nindex_transcripts = false\n\n[hook]\nthreshold = 0.5\n",
    );
    let etag = view.0["etag"].as_str().unwrap().to_string();
    let fixture = fixture(view);
    let (window, _, settings) = open(cx, fixture.clone(), RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    let sources = format!("sources = [ \"{}\" ]", folder.path());
    assert_eq!(
        raw_value(cx, &settings),
        format!("index_transcripts = false\n{sources}\n\n[hook]\nthreshold = 0.5\n"),
        "the editor mirrors the file's keys, sorted as the web receives them, without the comment"
    );

    let edited = [
        "index_transcripts = true",
        sources.as_str(),
        "",
        "[hook]",
        "",
        "[mcp]",
        "max_tokens = 900",
        "",
    ];
    replace_raw(cx, window, &edited);
    assert_eq!(raw_value(cx, &settings), edited.join("\n"));
    assert!(
        settings.read_with(cx, |s, _| s.pending().is_empty()),
        "typing alone changes nothing"
    );
    click(cx, window, "raw-apply");
    render(cx, window);

    let pending = settings.read_with(cx, |s, _| s.pending().clone());
    assert_eq!(
        pending.paths(),
        ["index_transcripts", "hook.threshold", "mcp"],
        "a diff against the file, in the order the text holds it"
    );
    assert_eq!(pending.len(), 3);
    assert_eq!(
        pending.get("index_transcripts"),
        Some(Some(&Value::Boolean(true)))
    );
    assert_eq!(pending.get("hook.threshold"), Some(None));
    let mut mcp = toml::Table::new();
    mcp.insert("max_tokens".into(), Value::Integer(900));
    assert_eq!(pending.get("mcp"), Some(Some(&Value::Table(mcp))));
    assert!(settings.read_with(cx, |s, _| s.raw_local().draft.is_none()));
    assert_eq!(
        raw_value(cx, &settings),
        format!("index_transcripts = true\n{sources}\n\n[hook]\n[mcp]\nmax_tokens = 900\n"),
        "the editor goes back to mirroring the file with the applied edits"
    );
    assert!(shown(cx, window, "save-bar-checking"));
    wait(cx, window, CHECK_DEBOUNCE);
    assert!(!shown(cx, window, "save-bar-checking"));
    let sent = super::tests::checks(&fixture);
    let last = sent.last().expect("a config check went out");
    assert!(
        last.contains("index_transcripts")
            && last.contains("hook.threshold")
            && last.contains("mcp"),
        "the applied rows are checked like form edits: {last}"
    );
    assert_eq!(
        label(cx, window, "save-bar-status").as_deref(),
        Some("3 unsaved changes")
    );

    let saved = rhebok::dashboard::core::config::save(&folder.config(), pending.to_save(&etag));
    assert!(saved.is_ok(), "{saved:?}");
    let written = std::fs::read_to_string(folder.config()).unwrap();
    assert!(written.contains("# keep me"), "{written}");
    assert!(written.contains("index_transcripts = true"), "{written}");
    assert!(!written.contains("threshold"), "{written}");
    assert!(written.contains("max_tokens = 900"), "{written}");

    settings.update(cx, |s, cx| {
        s.set("index_transcripts", Some(Value::Boolean(false)), cx)
    });
    render(cx, window);
    assert_eq!(
        settings.read_with(cx, |s, _| s.pending().paths()),
        ["hook.threshold", "mcp"],
        "a form edit back to the file's value cancels the applied row"
    );
}

#[gpui_kit::test]
fn a_syntax_error_is_shown_and_nothing_is_applied(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(raw_file(&folder, "index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture.clone(), RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    replace_raw(cx, window, &["index_transcripts = = true", ""]);
    click(cx, window, "raw-apply");
    render(cx, window);

    let shown = label(cx, window, "raw-problem").expect("the error is shown");
    assert!(
        shown.starts_with("TOML syntax error: Invalid TOML document: "),
        "{shown}"
    );
    assert!(
        shown.contains("1:  index_transcripts = = true\n"),
        "the message carries smol-toml's code block: {shown}"
    );
    assert!(settings.read_with(cx, |s, _| s.pending().is_empty()));
    assert_eq!(
        raw_value(cx, &settings),
        "index_transcripts = = true\n",
        "the draft is kept so it can be fixed"
    );
    assert!(!shown_save_bar(cx, window));

    replace_raw(cx, window, &["index_transcripts = true", ""]);
    click(cx, window, "raw-apply");
    render(cx, window);
    assert!(!super::tests::shown(cx, window, "raw-problem"));
    assert_eq!(
        settings.read_with(cx, |s, _| s.pending().len()),
        2,
        "sources removed too"
    );
}

fn shown_save_bar(cx: &mut TestAppContext, window: WindowHandle<Root>) -> bool {
    shown(cx, window, "save-bar")
}

#[gpui_kit::test]
fn revert_restores_the_file_text(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(raw_file(&folder, "index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture.clone(), RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    let original = raw_value(cx, &settings);
    replace_raw(cx, window, &["index_transcripts = = true", ""]);
    click(cx, window, "raw-apply");
    render(cx, window);
    assert!(shown(cx, window, "raw-problem"));

    click(cx, window, "raw-revert");
    render(cx, window);
    assert_eq!(raw_value(cx, &settings), original);
    assert!(settings.read_with(cx, |s, _| s.raw_local().draft.is_none()));
    assert!(!shown(cx, window, "raw-problem"), "revert clears the error");
    assert!(settings.read_with(cx, |s, _| s.pending().is_empty()));

    settings.update(cx, |s, cx| {
        s.set("index_transcripts", Some(Value::Boolean(true)), cx)
    });
    render(cx, window);
    assert!(
        raw_value(cx, &settings).starts_with("index_transcripts = true\n"),
        "with no draft the editor follows form edits"
    );

    replace_raw(cx, window, &["index_transcripts = = true", ""]);
    section(cx, window, &settings, Section::Sources);
    section(cx, window, &settings, Section::Advanced);
    assert!(
        settings.read_with(cx, |s, _| s.raw_local().draft.is_none()),
        "leaving the section drops the draft, as the web unmounts it"
    );
    assert!(raw_value(cx, &settings).starts_with("index_transcripts = true\n"));

    click(cx, window, "raw-copy");
    assert_eq!(label(cx, window, "raw-copy").as_deref(), Some("Copied"));
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()),
        Some(raw_value(cx, &settings))
    );
    wait(cx, window, Duration::from_millis(1499));
    assert_eq!(label(cx, window, "raw-copy").as_deref(), Some("Copied"));
    wait(cx, window, Duration::from_millis(1));
    assert_eq!(label(cx, window, "raw-copy").as_deref(), Some("Copy"));
}

#[gpui_kit::test]
fn the_raw_editor_never_shows_a_secret(cx: &mut TestAppContext) {
    let secret = "sk-raw-never-shown";
    let folder = Folder::new();
    let fixture = fixture(raw_file(
        &folder,
        &format!(
            "index_transcripts = false\n\n[embed]\ntoken = \"{secret}\"\napi_key = \"{secret}\"\n"
        ),
    ));
    let (window, _, settings) = open(cx, fixture, RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    let text = raw_value(cx, &settings);
    assert!(text.contains("index_transcripts = false"), "{text}");
    assert!(!text.contains(secret), "{text}");
    assert!(!text.contains("token"), "{text}");
}

#[gpui_kit::test]
fn the_weights_table_marks_inherited_cells(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(raw_file(
        &folder,
        "index_transcripts = false\n\n[hook.weights]\nmarkdown = 0.9\n",
    ));
    let (window, _, settings) = open(cx, fixture.clone(), RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    let tag = |cx: &mut TestAppContext, path: &str, kind: &str| {
        label(
            cx,
            window,
            &format!("{}-{kind}", super::nav::field_id(path)),
        )
    };
    assert_eq!(
        label(cx, window, "weights-row-markdown").as_deref(),
        Some("markdown (1)")
    );
    let name = super::tests::bounds(cx, window, "weights-row-markdown");
    let cells = [
        "weights.markdown",
        "hook.weights.markdown",
        "mcp.weights.markdown",
    ]
    .map(|p| super::tests::bounds(cx, window, &super::nav::field_id(p)));
    assert_eq!(
        f32::from(cells[0].left() - name.left()),
        156.,
        "a 140px name column, then the 16px column gap"
    );
    let card = super::tests::bounds(cx, window, "card-weights");
    let track = (f32::from(card.size.width) - 140. - 3. * 16.) / 3.;
    for cell in cells {
        assert!(
            (f32::from(cell.size.width) - track).abs() <= 0.5,
            "three equal 1fr columns, each within the device px GPUI lays out on: {cell:?}, {track}"
        );
    }
    assert_eq!(
        tag(cx, "hook.weights.pdf", "inherited").as_deref(),
        Some("inherits")
    );
    assert_eq!(
        tag(cx, "mcp.weights.pdf", "inherited").as_deref(),
        Some("inherits")
    );
    assert_eq!(
        tag(cx, "mcp.weights.transcript", "inherited").as_deref(),
        Some("shipped default"),
        "MCP's transcript weight is its own default while Global transcript is the shipped one"
    );
    assert_eq!(
        tag(cx, "hook.weights.markdown", "inherited"),
        None,
        "set in the file"
    );
    assert_eq!(
        tag(cx, "weights.pdf", "inherited"),
        None,
        "Global never inherits"
    );
    assert_eq!(
        input_text(cx, &settings, "hook.weights.markdown").as_deref(),
        Some("0.9")
    );
    let placeholder = settings.read_with(cx, |s, cx| {
        s.input("hook.weights.pdf")
            .map(|i| i.read(cx).presentation().placeholder().to_string())
    });
    assert_eq!(placeholder.as_deref(), Some("1"), "the greyed value in use");

    type_into(cx, window, "input-hook.weights.pdf", "0.4");
    render(cx, window);
    assert_eq!(
        pending(cx, &settings, "hook.weights.pdf"),
        Some(Value::Float(0.4))
    );
    assert_eq!(
        tag(cx, "hook.weights.pdf", "edited").as_deref(),
        Some("edited")
    );
    assert_eq!(tag(cx, "hook.weights.pdf", "inherited"), None);
    type_into(cx, window, "input-weights.transcript", "5");
    render(cx, window);
    assert_eq!(
        tag(cx, "weights.transcript", "edited").as_deref(),
        Some("edited")
    );

    let folder = Folder::new();
    let fixture = fixture_with(raw_file(
        &folder,
        "index_transcripts = false\n\n[weights]\ntranscript = 0.5\n",
    ));
    let (window, _, settings) = open(cx, fixture, RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    assert_eq!(
        label(
            cx,
            window,
            &format!(
                "{}-inherited",
                super::nav::field_id("mcp.weights.transcript")
            )
        )
        .as_deref(),
        Some("inherits"),
        "any other Global value reaches both surfaces"
    );
}

fn fixture_with(view: ConfigView) -> Arc<FixtureBackend> {
    super::tests::fixture(view)
}

#[gpui_kit::test]
fn copy_on_an_unparseable_file_copies_nothing_and_still_says_copied(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(raw_file(&folder, "index_transcripts = = false\n"));
    let (window, _, settings) = open(cx, fixture, RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    assert!(shown(cx, window, "raw-unparseable"));
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("before".into()));
    click(cx, window, "raw-copy");
    render(cx, window);
    assert_eq!(label(cx, window, "raw-copy").as_deref(), Some("Copied"));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default(),
        "",
        "the web writes its empty mirror over what the clipboard held"
    );
}

#[gpui_kit::test]
fn the_weights_inputs_paint_on_chromes_fractional_columns(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(raw_file(&folder, "index_transcripts = false\n"));
    let (window, _, settings) = open(cx, fixture.clone(), RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    let painted = |cx: &mut TestAppContext, path: &str| {
        let id = SharedString::from(format!("input-{path}"));
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let line = crate::theme::tokens(cx).line;
            let input = window.find(id).bounds();
            let (top, left) = (f32::from(input.top()) * 2., f32::from(input.left()) * 2.);
            window
                .painted_quads()
                .into_iter()
                .filter(|q| q.border_color == line && q.border_widths.left.0 > 0.)
                .filter(|q| (q.bounds.origin.y.0 - top).abs() <= 2.)
                .filter(|q| (q.bounds.origin.x.0 - left).abs() <= 4.)
                .map(|q| {
                    (
                        q.bounds.origin.x.0,
                        q.bounds.origin.x.0 + q.bounds.size.width.0,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap()
    };
    assert_eq!(
        [
            painted(cx, "weights.markdown"),
            painted(cx, "hook.weights.markdown"),
            painted(cx, "mcp.weights.markdown"),
        ],
        [
            vec![(838., 1018.)],
            vec![(1476., 1656.)],
            vec![(2112., 2292.)],
        ],
        "Chrome lays the 1fr columns out at x=419, 737.664 and 1056.328 and paints each input over whole CSS pixels"
    );
}

#[gpui_kit::test]
fn a_ringed_weights_cell_rings_the_exact_column_its_input_paints_in(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let fixture = fixture(raw_file(&folder, "index_transcripts = false\n"));
    let (window, shell, settings) = open(cx, fixture.clone(), RAW_HEIGHT);
    section(cx, window, &settings, Section::Advanced);
    shell.update(cx, |s, cx| s.open_setting("mcp.weights.markdown", cx));
    render(cx, window);
    let mut rings = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let warn = crate::theme::tokens(cx).warn;
            window
                .painted_quads()
                .into_iter()
                .filter(|q| q.border_color == warn && q.border_widths.left.0 > 0.)
                .map(|q| {
                    (
                        q.bounds.origin.x.0,
                        q.bounds.origin.x.0 + q.bounds.size.width.0,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap();
    rings.dedup();
    assert_eq!(
        rings,
        [(2108., 2722.)],
        "the MCP column is 1056.333..1359 in Chrome, so its 2px ring sits 2 CSS px outside the input frame at 2112"
    );
}
