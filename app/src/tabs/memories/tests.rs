use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, size, AppContext, Bounds, Entity, Point, SharedString, TestAppContext, WindowBounds,
    WindowHandle, WindowOptions,
};
use rhebok::dashboard::core::memories::{
    Memories as MemoryList, MemoryReach, MemoryView, ReachedDocument,
};
use rhebok::dashboard::core::memory_write::{DeleteOutcome, SaveOutcome};
use rhebok::dashboard::core::stats::{Stats, Tiers};
use rhebok::dashboard::core::DashboardError;
use rhebok::memory::{Memory, MemoryFacts, MemoryKind, Origin};

use super::list::row_id;
use super::{
    clip, confidence_of, refusal, saved_text, scope_of, Confirming, Draft, Leave, Memories,
    BAD_CONFIDENCE, EMPTY_PATH,
};
use crate::backend::{self, FixtureBackend};
use crate::shell::{Shell, TabId};
use crate::theme;
use crate::version_gate::Gate;

fn stats() -> Stats {
    Stats {
        documents: 4,
        chunks: 5,
        by_source: HashMap::new(),
        sessions_by_agent: BTreeMap::new(),
        model: None,
        skipped: Vec::new(),
        skipped_ignored: 0,
        bench: None,
        bench_provenance: None,
        bench_at: None,
        tiers: Tiers { hook: 1, mcp: 2 },
        hook_threshold: 0.68,
        memory: BTreeMap::new(),
    }
}

fn memory(id: &str, kind: MemoryKind, text: &str, created: i64) -> MemoryView {
    MemoryView {
        memory: Memory {
            id: id.to_string(),
            uri: format!("memory://{id}"),
            doc_id: format!("doc-{id}"),
            title: format!("Title {id}"),
            text: text.to_string(),
            facts: MemoryFacts {
                kind,
                created,
                project: None,
                origin: Origin::User,
                confidence: 100,
                session: None,
                source_hash: None,
                source_stamp: None,
            },
        },
        project_name: None,
    }
}

fn two() -> MemoryList {
    MemoryList {
        memories: vec![
            memory(
                "aaa111",
                MemoryKind::Lesson,
                "Feed the starter twice a day.",
                100,
            ),
            memory("bbb222", MemoryKind::Fact, "The oven runs hot.", 200),
        ],
        unavailable: None,
    }
}

fn fixture(list: MemoryList) -> Arc<FixtureBackend> {
    let fixture = FixtureBackend::default();
    fixture.stats.set(Ok(stats()));
    fixture.memories.set(list);
    Arc::new(fixture)
}

fn open(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
) -> (WindowHandle<Root>, Entity<Shell>, Entity<Memories>) {
    let (window, shell) = cx.update(|cx| {
        if !cx.has_global::<theme::Tokens>() {
            theme::install(cx);
        }
        backend::install(cx, fixture);
        let (window, shell) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1400.), px(900.)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| Shell::new(Gate::Ok, cx)),
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Base Root"), shell)
    });
    cx.run_until_parked();
    click(cx, window, TabId::Memories.button_id());
    let memories = shell
        .read_with(cx, |s, _| s.memories.clone())
        .expect("memories opened");
    render(cx, window);
    (window, shell, memories)
}

fn render(cx: &mut TestAppContext, window: WindowHandle<Root>) {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

fn click(cx: &mut TestAppContext, window: WindowHandle<Root>, id: impl Into<SharedString>) {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn click_now(cx: &mut TestAppContext, window: WindowHandle<Root>, id: impl Into<SharedString>) {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

fn label(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> Option<String> {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(id)
            .map(|e| e.label().unwrap_or_default().to_string())
    })
    .unwrap()
}

fn settle(cx: &mut TestAppContext) {
    let executor = cx.executor();
    let dispatcher = executor.dispatcher().as_test().expect("test dispatcher");
    while dispatcher.tick(true) {}
    cx.run_until_parked();
}

fn requests_named(fixture: &FixtureBackend, prefix: &str) -> Vec<String> {
    fixture
        .requests()
        .into_iter()
        .filter(|r| r.starts_with(prefix))
        .collect()
}

fn selected(cx: &mut TestAppContext, memories: &Entity<Memories>) -> Option<String> {
    memories.read_with(cx, |m, _| m.selected_id().map(str::to_string))
}

#[gpui_kit::test]
fn the_list_shows_every_memory_newest_first_and_counts_the_filter(cx: &mut TestAppContext) {
    let (window, _, memories) = open(cx, fixture(two()));
    assert_eq!(
        memories.read_with(cx, |m, _| m.shown_ids()),
        ["bbb222", "aaa111"]
    );
    assert_eq!(label(cx, window, "memories-count").as_deref(), Some("2"));
    assert_eq!(
        label(cx, window, &row_id("aaa111")).as_deref(),
        Some("Feed the starter twice a day.")
    );
    click(cx, window, "memories-kind-2");
    assert_eq!(memories.read_with(cx, |m, _| m.shown_ids()), ["bbb222"]);
    assert_eq!(
        label(cx, window, "memories-count").as_deref(),
        Some("1 of 2")
    );
    memories.update(cx, |m, cx| m.set_kind_filter(0, cx));
    memories.update(cx, |m, cx| m.set_needle("AAA1".into(), cx));
    assert_eq!(memories.read_with(cx, |m, _| m.shown_ids()), ["aaa111"]);
}

#[gpui_kit::test]
fn an_empty_store_says_so_and_offers_new(cx: &mut TestAppContext) {
    let (window, _, _) = open(
        cx,
        fixture(MemoryList {
            memories: Vec::new(),
            unavailable: None,
        }),
    );
    assert_eq!(
        label(cx, window, "memories-empty").as_deref(),
        Some("No memories yet — write one with rhebok memory add or the New button.")
    );
    assert_eq!(label(cx, window, "memories-count").as_deref(), Some("0"));
}

#[gpui_kit::test]
fn a_disabled_store_shows_its_reason_and_try_again_reloads(cx: &mut TestAppContext) {
    let disabled = fixture(MemoryList {
        memories: Vec::new(),
        unavailable: Some(rhebok::memory::DISABLED.to_string()),
    });
    let (window, _, _) = open(cx, disabled.clone());
    assert_eq!(
        label(cx, window, "memories-unavailable").as_deref(),
        Some(rhebok::memory::DISABLED)
    );
    assert!(label(cx, window, "memories-new").is_none());
    let listed = disabled.memories.calls();
    disabled.memories.set(two());
    click(cx, window, "memories-retry");
    assert_eq!(disabled.memories.calls(), listed + 1);
    assert!(label(cx, window, "memories-unavailable").is_none());
    assert_eq!(label(cx, window, "memories-count").as_deref(), Some("2"));
}

#[gpui_kit::test]
fn a_save_in_flight_locks_rows_new_and_close(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_save.set(Ok(SaveOutcome::Saved {
        id: "aaa111".into(),
    }));
    let (window, shell, memories) = open(cx, backend.clone());
    click(cx, window, row_id("aaa111"));
    assert_eq!(selected(cx, &memories).as_deref(), Some("aaa111"));
    memories.update(cx, |m, cx| {
        m.set_text("Feed the starter three times a day.".into(), cx)
    });
    let release = backend.memory_save.hold();
    click_now(cx, window, "editor-save");
    assert!(memories.read_with(cx, |m, _| m.writing()));
    let busy = label(cx, window, "memories-busy").unwrap();
    assert!(
        busy.starts_with(
            "Saving “Feed the starter three times a day.”. Other memories, New and close are locked"
        ),
        "{busy}"
    );
    assert_eq!(label(cx, window, "editor-save").as_deref(), Some("Saving…"));

    click_now(cx, window, row_id("bbb222"));
    click_now(cx, window, "memories-new");
    click_now(cx, window, "editor-close");
    memories.update(cx, |m, cx| {
        m.request(Leave::Pick("bbb222".into()), cx);
        m.request(Leave::New, cx);
        m.request(Leave::Close, cx);
        m.load_reach(cx);
    });
    memories.read_with(cx, |m, _| {
        assert_eq!(m.selected_id(), Some("aaa111"));
        assert!(
            m.leaving().is_none(),
            "a locked panel must not even ask to discard"
        );
        assert_eq!(
            m.draft().unwrap().text,
            "Feed the starter three times a day."
        );
        assert!(m.reach().is_none() && m.reach_error().is_none());
    });
    assert!(requests_named(&backend, "memory_reach").is_empty());
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    let editable = memories.read_with(cx, |m, cx| {
        m.inputs.text.as_ref().unwrap().read(cx).is_editable()
    });
    assert!(
        !editable,
        "the text box is read-only while a write is in flight"
    );

    release.send(()).unwrap();
    settle(cx);
    render(cx, window);
    memories.read_with(cx, |m, _| {
        assert!(!m.writing() && m.busy().is_none());
        assert_eq!(m.notice().unwrap().text, "Saved as aaa111.");
    });
    assert_eq!(shell.read_with(cx, |s, _| s.memory_writes()), 1);
    let editable = memories.read_with(cx, |m, cx| {
        m.inputs.text.as_ref().unwrap().read(cx).is_editable()
    });
    assert!(editable);
    click(cx, window, row_id("bbb222"));
    assert_eq!(selected(cx, &memories).as_deref(), Some("bbb222"));
}

#[gpui_kit::test]
fn a_write_that_returns_after_the_panel_moved_reports_a_notice_instead(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_save.set(Err(DashboardError::Unavailable(
        "embedding unavailable".into(),
    )));
    let (window, _, memories) = open(cx, backend.clone());
    click(cx, window, row_id("aaa111"));
    memories.update(cx, |m, cx| m.set_text("Feed it at dawn.".into(), cx));
    let release = backend.memory_save.hold();
    click_now(cx, window, "editor-save");
    memories.update(cx, |m, cx| m.set_text("Feed it at dusk.".into(), cx));
    release.send(()).unwrap();
    settle(cx);
    memories.read_with(cx, |m, _| {
        assert_eq!(m.panel_error(), None);
        let notice = m.notice().expect("a notice");
        assert_eq!(
            notice.text,
            "“Feed it at dawn.” was not saved: embedding unavailable"
        );
        assert!(notice.warn);
        assert_eq!(m.draft().unwrap().text, "Feed it at dusk.");
    });

    backend.memory_save.set(Ok(SaveOutcome::Saved {
        id: "aaa111".into(),
    }));
    let release = backend.memory_save.hold();
    click_now(cx, window, "editor-save");
    memories.update(cx, |m, cx| m.set_text("Feed it at noon.".into(), cx));
    release.send(()).unwrap();
    settle(cx);
    memories.read_with(cx, |m, _| {
        assert_eq!(m.notice().unwrap().text, "Saved as aaa111.");
        assert_eq!(
            m.draft().unwrap().text,
            "Feed it at noon.",
            "the moved panel is not reopened"
        );
    });

    backend.memory_save.set(Err(DashboardError::Unavailable(
        "embedding unavailable".into(),
    )));
    click(cx, window, "editor-save");
    memories.read_with(cx, |m, _| {
        assert_eq!(m.panel_error(), Some("embedding unavailable"));
        assert!(m.notice().is_none());
    });
}

#[gpui_kit::test]
fn delete_sends_the_id_captured_when_delete_was_pressed(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_delete.set(Ok(DeleteOutcome {
        id: "aaa111".into(),
        title: "Title aaa111".into(),
    }));
    let (window, shell, memories) = open(cx, backend.clone());
    click(cx, window, row_id("aaa111"));
    click(cx, window, "editor-delete");
    assert_eq!(
        memories.read_with(cx, |m, _| m.confirming().cloned()),
        Some(Confirming {
            id: "aaa111".into(),
            title: "Title aaa111".into()
        })
    );
    assert_eq!(
        label(cx, window, "delete-confirm").as_deref(),
        Some(
            "Delete “Title aaa111” (aaa111)? A memory exists nowhere else, so this cannot be undone."
        )
    );
    assert!(label(cx, window, "editor-delete").is_none());
    memories.update(cx, |m, cx| {
        m.selected_id = Some("bbb222".into());
        cx.notify();
    });
    click(cx, window, "delete-it");
    assert!(requests_named(&backend, "memory_delete").is_empty());
    memories.read_with(cx, |m, _| {
        assert!(m.confirming().is_none());
        assert_eq!(
            m.notice().unwrap().text,
            "Nothing was deleted: the panel no longer shows “Title aaa111”, the memory you confirmed."
        );
    });

    memories.update(cx, |m, cx| {
        m.selected_id = Some("aaa111".into());
        cx.notify();
    });
    click(cx, window, "editor-delete");
    memories.update(cx, |m, cx| {
        m.selected_id = Some("bbb222".into());
        m.set_text("an edit made while the confirmation is open".into(), cx);
        m.selected_id = Some("aaa111".into());
        cx.notify();
    });
    assert_eq!(
        memories.read_with(cx, |m, _| m.confirming().map(|c| c.id.clone())),
        Some("aaa111".into()),
        "the confirmation keeps the id captured when Delete was pressed"
    );
    click(cx, window, "delete-it");
    assert_eq!(
        requests_named(&backend, "memory_delete"),
        ["memory_delete DeleteRequest { id: \"aaa111\" }"]
    );
    memories.read_with(cx, |m, _| {
        assert!(
            m.draft().is_none(),
            "a delete closes the panel it came from"
        );
        assert_eq!(m.notice().unwrap().text, "Deleted “Title aaa111”.");
    });
    assert_eq!(shell.read_with(cx, |s, _| s.memory_writes()), 1);
}

#[gpui_kit::test]
fn opening_another_memory_drops_a_pending_delete(cx: &mut TestAppContext) {
    let backend = fixture(two());
    let (window, _, memories) = open(cx, backend.clone());
    click(cx, window, row_id("aaa111"));
    click(cx, window, "editor-delete");
    click(cx, window, row_id("bbb222"));
    memories.read_with(cx, |m, _| {
        assert_eq!(m.selected_id(), Some("bbb222"));
        assert!(m.confirming().is_none());
    });
    assert!(label(cx, window, "delete-it").is_none());
    assert!(requests_named(&backend, "memory_delete").is_empty());
}

#[gpui_kit::test]
fn a_duplicate_outcome_leaves_the_draft_as_typed(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_save.set(Ok(SaveOutcome::Duplicate {
        id: "aaa111".into(),
    }));
    let (window, shell, memories) = open(cx, backend.clone());
    click(cx, window, "memories-new");
    memories.update(cx, |m, cx| {
        m.set_text("Feed the starter twice daily.".into(), cx)
    });
    click(cx, window, "editor-save");
    memories.read_with(cx, |m, _| {
        let draft = m.draft().unwrap();
        assert_eq!(draft.text, "Feed the starter twice daily.");
        assert_eq!(draft.id, None);
        assert_eq!(m.selected_id(), None);
        assert!(m.dirty());
    });
    assert_eq!(
        label(cx, window, "memories-notice").as_deref(),
        Some("Nothing was written: a very similar memory already exists as aaa111. What you typed is still in the panel.")
    );
    assert_eq!(
        shell.read_with(cx, |s, _| s.memory_writes()),
        0,
        "a duplicate wrote nothing"
    );

    click(cx, window, "notice-open");
    memories.read_with(cx, |m, _| {
        assert_eq!(m.leaving(), Some(&Leave::Pick("aaa111".into())));
        assert_eq!(m.selected_id(), None, "Open on a dirty draft asks first");
    });
    assert_eq!(
        label(cx, window, "discard-prompt").as_deref(),
        Some("Discard the new lesson you were writing, “Feed the starter twice daily.”, and open aaa111, “Title aaa111”? Nothing is written until you press Save.")
    );
    click(cx, window, "discard");
    memories.read_with(cx, |m, _| {
        assert_eq!(m.selected_id(), Some("aaa111"));
        assert!(
            m.notice().is_none(),
            "opening the memory the notice named retires the notice"
        );
    });
}

#[gpui_kit::test]
fn the_global_checkbox_disables_the_path(cx: &mut TestAppContext) {
    let (window, _, memories) = open(cx, fixture(two()));
    click(cx, window, "memories-new");
    let path_editable = |cx: &mut TestAppContext| {
        render(cx, window);
        memories.read_with(cx, |m, cx| {
            m.inputs.path.as_ref().unwrap().read(cx).is_editable()
        })
    };
    assert!(!path_editable(cx));
    click(cx, window, "editor-global");
    assert!(!memories.read_with(cx, |m, _| m.draft().unwrap().global));
    assert!(path_editable(cx));
    click(cx, window, "editor-global");
    assert!(memories.read_with(cx, |m, _| m.draft().unwrap().global));
    assert!(!path_editable(cx));
}

#[gpui_kit::test]
fn validation_refuses_an_empty_path_and_an_out_of_range_confidence(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_save.set(Ok(SaveOutcome::Saved {
        id: "ccc333".into(),
    }));
    let (window, _, memories) = open(cx, backend.clone());
    click(cx, window, "memories-new");
    memories.update(cx, |m, cx| {
        m.set_text("Proof for twelve hours.".into(), cx);
        m.set_global(false, cx);
    });
    click(cx, window, "editor-save");
    assert_eq!(
        label(cx, window, "editor-error").as_deref(),
        Some(EMPTY_PATH)
    );
    assert_eq!(
        EMPTY_PATH,
        "Not saved: global is unticked but the project path is empty. Enter an absolute project path, or tick global."
    );
    for bad in ["101", "abc", "", "-1", "1.5", "99999999999999999999"] {
        memories.update(cx, |m, cx| {
            m.set_path("/home/me/bakery".into(), cx);
            m.set_confidence(bad.into(), cx);
        });
        click(cx, window, "editor-save");
        assert_eq!(
            label(cx, window, "editor-error").as_deref(),
            Some(BAD_CONFIDENCE),
            "{bad:?}"
        );
    }
    assert_eq!(
        BAD_CONFIDENCE,
        "Not saved: confidence must be a whole number from 0 to 100."
    );
    assert!(requests_named(&backend, "memory_save").is_empty());
    memories.update(cx, |m, cx| m.set_confidence("0100".into(), cx));
    click(cx, window, "editor-save");
    let sent = requests_named(&backend, "memory_save");
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].contains("scope: \"/home/me/bakery\"") && sent[0].contains("confidence: Some(100)"),
        "{}",
        sent[0]
    );
}

#[gpui_kit::test]
fn reach_is_fetched_for_the_open_memory_only_when_asked(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_reach.set(Ok(MemoryReach {
        reach: Some(vec![ReachedDocument {
            doc_id: "d1".into(),
            title: "Sourdough starter".into(),
            uri: "file:///vault/sourdough.md".into(),
            relevance: 0.815,
        }]),
        error: None,
    }));
    let (window, _, _) = open(cx, backend.clone());
    click(cx, window, row_id("aaa111"));
    assert!(requests_named(&backend, "memory_reach").is_empty());
    click(cx, window, "editor-reach");
    assert_eq!(
        requests_named(&backend, "memory_reach"),
        ["memory_reach aaa111"]
    );
    assert_eq!(
        label(cx, window, "reach-0").as_deref(),
        Some("Sourdough starter 0.81")
    );
}

#[test]
fn the_helpers_follow_the_web_rules() {
    let mut d = Draft::fresh();
    assert_eq!(refusal(&d), None);
    d.global = false;
    d.scope = "  ".into();
    assert_eq!(refusal(&d), Some(EMPTY_PATH));
    assert_eq!(confidence_of("007"), Some(7));
    assert_eq!(confidence_of("100"), Some(100));
    assert_eq!(confidence_of("101"), None);
    assert_eq!(clip("\n  first line here\nsecond", 5), "first…");
    let replaced = SaveOutcome::Replaced {
        id: "n".into(),
        previous_title: "Old".into(),
    };
    assert_eq!(
        saved_text(&replaced, None),
        "Saved as n, and it REPLACED an existing memory that said nearly the same thing: “Old”. That earlier memory is gone."
    );
    assert_eq!(
        saved_text(&replaced, Some("o")),
        "Saved your edit as n (it was o)."
    );
    assert_eq!(
        saved_text(&SaveOutcome::Saved { id: "n".into() }, Some("n")),
        "Saved as n."
    );
    let mut scoped = memory("p", MemoryKind::Fact, "x", 1);
    scoped.memory.facts.project = Some("/srv/bakery".into());
    assert_eq!(scope_of(&scoped), "/srv/bakery");
    scoped.project_name = Some("bakery".into());
    assert_eq!(scope_of(&scoped), "bakery · /srv/bakery");
}

#[gpui_kit::test]
fn a_create_that_replaced_another_memory_is_a_warning(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_save.set(Ok(SaveOutcome::Replaced {
        id: "aaa111".into(),
        previous_title: "Title old".into(),
    }));
    let (window, _, memories) = open(cx, backend.clone());
    click(cx, window, "memories-new");
    memories.update(cx, |m, cx| {
        m.set_text("Feed the starter twice a day.".into(), cx)
    });
    click(cx, window, "editor-save");
    memories.read_with(cx, |m, _| {
        let notice = m.notice().unwrap();
        assert_eq!(
            notice.text,
            "Saved as aaa111, and it REPLACED an existing memory that said nearly the same thing: “Title old”. That earlier memory is gone."
        );
        assert!(notice.warn, "a create that replaced another memory warns");
    });
    assert_eq!(label(cx, window, "memories-notice").map(|_| ()), Some(()));

    click(cx, window, row_id("bbb222"));
    memories.update(cx, |m, cx| m.set_text("The oven runs very hot.".into(), cx));
    click(cx, window, "editor-save");
    memories.read_with(cx, |m, _| {
        let notice = m.notice().unwrap();
        assert_eq!(notice.text, "Saved your edit as aaa111 (it was bbb222).");
        assert!(
            !notice.warn,
            "an edit that replaced its own memory is not a warning"
        );
    });
}

#[gpui_kit::test]
fn a_delete_that_returns_after_the_panel_moved_reports_a_notice_instead(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_delete.set(Err(DashboardError::Conflict(
        "the memory store is busy".into(),
    )));
    let (window, _, memories) = open(cx, backend.clone());
    click(cx, window, row_id("aaa111"));
    click(cx, window, "editor-delete");
    let release = backend.memory_delete.hold();
    click_now(cx, window, "delete-it");
    assert_eq!(label(cx, window, "delete-it").as_deref(), Some("Deleting…"));
    memories.update(cx, |m, cx| m.set_text("moved on".into(), cx));
    release.send(()).unwrap();
    settle(cx);
    memories.read_with(cx, |m, _| {
        assert_eq!(m.panel_error(), None);
        let notice = m.notice().unwrap();
        assert_eq!(
            notice.text,
            "“Title aaa111” was not deleted: the memory store is busy"
        );
        assert!(notice.warn);
    });

    backend.memory_delete.set(Ok(DeleteOutcome {
        id: "aaa111".into(),
        title: "Title aaa111".into(),
    }));
    memories.update(cx, |m, cx| m.press_delete(cx));
    let release = backend.memory_delete.hold();
    click_now(cx, window, "delete-it");
    memories.update(cx, |m, cx| m.set_text("moved again".into(), cx));
    release.send(()).unwrap();
    settle(cx);
    memories.read_with(cx, |m, _| {
        assert_eq!(m.notice().unwrap().text, "Deleted “Title aaa111”.");
        assert_eq!(
            m.draft().map(|d| d.text.as_str()),
            Some("moved again"),
            "a delete does not close a panel that moved while it ran"
        );
    });
}

#[gpui_kit::test]
fn a_duplicate_after_the_panel_moved_does_not_claim_the_draft_is_kept(cx: &mut TestAppContext) {
    let backend = fixture(two());
    backend.memory_save.set(Ok(SaveOutcome::Duplicate {
        id: "aaa111".into(),
    }));
    let (window, _, memories) = open(cx, backend.clone());
    click(cx, window, "memories-new");
    memories.update(cx, |m, cx| {
        m.set_text("Feed the starter twice daily.".into(), cx)
    });
    let release = backend.memory_save.hold();
    click_now(cx, window, "editor-save");
    memories.update(cx, |m, cx| {
        m.set_text("Something else entirely.".into(), cx)
    });
    release.send(()).unwrap();
    settle(cx);
    memories.read_with(cx, |m, _| {
        let notice = m.notice().unwrap();
        assert_eq!(
            notice.text,
            "Nothing was written: a very similar memory already exists as aaa111."
        );
        assert_eq!(notice.kept, None);
        assert_eq!(notice.open.as_deref(), Some("aaa111"));
    });
}

#[gpui_kit::test]
fn rows_keep_their_height_when_the_list_overflows(cx: &mut TestAppContext) {
    let many = MemoryList {
        memories: (0..40)
            .map(|i| memory(&format!("m{i:03}"), MemoryKind::Lesson, "A lesson.", i))
            .collect(),
        unavailable: None,
    };
    let (window, _, _) = open(cx, fixture(many));
    let heights: Vec<f32> = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            ["m039", "m020", "m000"]
                .map(|id| f32::from(window.find(row_id(id)).bounds().size.height))
                .to_vec()
        })
        .unwrap();
    assert_eq!(heights, [40., 40., 40.]);
}
