pub mod ignore_editor;
pub mod index_panel;
pub mod install_card;
pub mod memory_panel;
pub mod retrieval_panel;
pub mod skipped_panel;
pub mod version_card;

use std::time::Duration;

use gpui_kit::*;

use crate::widgets::baseline;
use rhebok::dashboard::core::stats::{Progress, Stats};

use crate::backend::run_backend;
use crate::shell::{Shell, TabId, TabShown};
use crate::theme::{tokens, Type};
use crate::widgets::banner;
use index_panel::{index_panel, IndexPanel};
use install_card::InstallCard;
use memory_panel::memory_panel;
use retrieval_panel::{retrieval_panel, RetrievalPanel};
use skipped_panel::SkippedPanel;
use version_card::VersionCard;

pub const POLL: Duration = Duration::from_secs(2);
pub const SINGLE_COLUMN_BELOW: f32 = 1024.;

pub struct Health {
    shell: WeakEntity<Shell>,
    stats: Option<Stats>,
    stats_error: Option<String>,
    visible: bool,
    cards_active: bool,
    progress: Progress,
    was_indexing: bool,
    poll: Option<Task<()>>,
    progress_task: Option<Task<()>>,
    progress_in_flight: bool,
    version: Entity<VersionCard>,
    install: Entity<InstallCard>,
    index: IndexPanel,
    retrieval: RetrievalPanel,
    skipped: Entity<SkippedPanel>,
    _subscriptions: [Subscription; 2],
}

impl Health {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Health>) -> Health {
        let subscriptions = [
            cx.subscribe(&shell, |health, _, shown: &TabShown, cx| {
                health.set_visible(shown.0 == TabId::Health, cx)
            }),
            cx.observe(&shell, |health, shell, cx| health.sync(&shell, cx)),
        ];
        let weak = shell.downgrade();
        let this = cx.entity().downgrade();
        cx.defer(move |cx| {
            let visible = shell.read(cx).tab() == TabId::Health;
            let _ = this.update(cx, |health, cx| {
                health.sync(&shell, cx);
                health.set_visible(visible, cx);
            });
        });
        Health {
            shell: weak.clone(),
            stats: None,
            stats_error: None,
            visible: false,
            cards_active: false,
            progress: Progress::Idle { idle: true },
            was_indexing: false,
            poll: None,
            progress_task: None,
            progress_in_flight: false,
            version: cx.new(|_| VersionCard::new()),
            install: cx.new(|_| InstallCard::new(weak.clone())),
            index: IndexPanel::default(),
            retrieval: RetrievalPanel::default(),
            skipped: cx.new(|_| SkippedPanel::new(weak)),
            _subscriptions: subscriptions,
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    pub fn version_card(&self) -> &Entity<VersionCard> {
        &self.version
    }

    pub fn install_card(&self) -> &Entity<InstallCard> {
        &self.install
    }

    fn sync(&mut self, shell: &Entity<Shell>, cx: &mut Context<Self>) {
        let shell = shell.read(cx);
        self.stats = shell.stats().cloned();
        self.stats_error = shell.stats_error().map(str::to_string);
        if let Some(stats) = &self.stats {
            let (skipped, hidden) = (stats.skipped.clone(), stats.skipped_ignored);
            self.skipped
                .update(cx, |panel, cx| panel.set_skipped(skipped, hidden, cx));
        }
        self.sync_cards(cx);
        cx.notify();
    }

    fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if visible == self.visible {
            return;
        }
        self.visible = visible;
        self.poll = visible.then(|| {
            cx.spawn(async move |this, cx| loop {
                if this
                    .update(cx, |health, cx| health.poll_progress(cx))
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(POLL).await;
            })
        });
        self.sync_cards(cx);
    }

    fn sync_cards(&mut self, cx: &mut Context<Self>) {
        let active = self.visible && self.stats.is_some() && self.stats_error.is_none();
        if active == self.cards_active {
            return;
        }
        self.cards_active = active;
        self.version
            .update(cx, |card, cx| card.set_active(active, cx));
        self.install
            .update(cx, |card, cx| card.set_active(active, cx));
    }

    fn poll_progress(&mut self, cx: &mut Context<Self>) {
        if self.progress_in_flight {
            return;
        }
        self.progress_in_flight = true;
        self.progress_task = Some(run_backend(
            cx,
            |backend| backend.progress(),
            |health: &mut Health, result, cx| {
                health.progress_in_flight = false;
                if let Ok(progress) = result {
                    health.set_progress(progress, cx);
                }
            },
        ));
    }

    fn set_progress(&mut self, progress: Progress, cx: &mut Context<Self>) {
        let indexing = progress.is_running();
        if self.was_indexing && !indexing {
            if let Some(shell) = self.shell.upgrade() {
                shell.update(cx, |shell, cx| shell.index_finished(cx));
            }
        }
        self.was_indexing = indexing;
        self.progress = progress;
        self.version
            .update(cx, |card, cx| card.set_indexing(indexing, cx));
    }

    fn index_now(&mut self, cx: &mut Context<Self>) {
        self.index.starting = true;
        self.index.note = None;
        self.index.task = Some(run_backend(
            cx,
            |backend| backend.index_start(false),
            |health: &mut Health, result, cx| {
                match result {
                    Ok(_) => {
                        health.index.note = None;
                        health.poll_progress(cx);
                    }
                    Err(e) => health.index.note = Some(e.to_string()),
                }
                health.index.starting = false;
            },
        ));
        cx.notify();
    }

    fn open_setting(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |shell, cx| shell.open_setting(key, cx));
        }
    }

    fn open_memories(&mut self, cx: &mut Context<Self>) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |shell, cx| shell.open_tab(TabId::Memories, cx));
        }
    }
}

fn column_track(fraction: f32) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_basis(px(0.))
        .flex_grow(fraction)
        .min_w(px(0.))
}

impl Render for Health {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let root = div()
            .id("health")
            .test_support()
            .aria_label("Health")
            .size_full();
        if let Some(error) = self.stats_error.clone() {
            return root
                .child(div().m(px(16.)).child(banner::error("health-error", error)))
                .into_any_element();
        }
        let Some(stats) = self.stats.clone() else {
            return root
                .child(
                    div()
                        .p(px(16.))
                        .size_base()
                        .text_color(t.muted)
                        .child(baseline::text("Loading…")),
                )
                .into_any_element();
        };
        let single_column = window.viewport_size().width < px(SINGLE_COLUMN_BELOW);
        let index = index_panel(
            &stats,
            &self.progress,
            &self.index,
            &t,
            cx.listener(|health, _, _, cx| health.index_now(cx)),
        );
        let memory = memory_panel(
            &stats,
            &t,
            cx.listener(|health, _, _, cx| health.open_memories(cx)),
        );
        let grid = if single_column {
            div()
                .id("health-grid")
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(index)
                .child(memory)
        } else {
            div()
                .id("health-grid")
                .flex()
                .gap(px(14.))
                .child(column_track(2.).child(index.flex_grow(1.)))
                .child(column_track(1.).child(memory.flex_grow(1.)))
        };
        let has_version = self.version.read(cx).has_version();
        let has_install = self.install.read(cx).has_install();
        let mut column = div()
            .id("health-scroll")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(14.))
            .pb(px(16.));
        if has_version {
            column = column.child(div().flex_none().child(self.version.clone()));
        }
        if has_install {
            column = column.child(div().flex_none().child(self.install.clone()));
        }
        let retrieval = retrieval_panel(&stats, &self.retrieval, &t, window, cx);
        root.child(
            column
                .child(grid.flex_none())
                .child(retrieval.flex_none())
                .child(div().flex_none().child(self.skipped.clone())),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests;
