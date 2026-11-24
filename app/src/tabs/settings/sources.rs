use std::collections::HashMap;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::fields::{
    card, code_line, number_field, shell, toggle_field, Ctx, Field, Hint, InputSpec, Part,
};
use super::list_editor::{list_editor, ListEditor, ListState};
use crate::tabs::health::index_panel::{IndexState, IndexStatus};
use crate::theme::{tokens, Face, Type, MONO, SM_FACE};
use crate::widgets::button;

pub const SOURCES_PLACEHOLDER: &str = "/absolute/path/to/notes or ~/notes";
pub const IGNORE_PLACEHOLDER: &str = "_templates, ~/notes/drafts or *.tmp";
pub const SOURCES_HINT: &str =
    "Folders are walked recursively; a single file works too. Changes apply at the next index.";
pub const IGNORE_HINT: &str = "Skipped in notes and sessions. A name skips any file or folder with that name, a path skips it and everything under it, and a glob such as *.tmp matches paths.";
pub const TRANSCRIPTS_HINT: &str = "Claude Code's sessions from ~/.claude/projects. Off turns off every agent's sessions, Codex's included.";
pub const MAX_AGE_HINT: &str = "Leave empty to index sessions of any age.";
pub const RELATIVE: &str = "Use an absolute path, or one that starts with ~/.";
pub const REFUSAL_TAIL: &str = ", and `rhebok index` refuses to run until it does";
pub const UNSAVED: &str = " You have unsaved changes; save them first if they should count.";
pub const MAX_AGE: &str = "index_transcripts_max_age_days";

const INDEX_CODE_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 12., 20.);

pub fn inputs() -> Vec<InputSpec> {
    vec![
        InputSpec::candidate("sources", SOURCES_PLACEHOLDER),
        InputSpec::candidate("ignore", IGNORE_PLACEHOLDER),
        InputSpec::number(MAX_AGE, true, None),
    ]
}

pub fn tail_of(path: &str) -> &str {
    path.strip_prefix('~').unwrap_or(path).trim_end_matches('/')
}

pub fn is_absolute(candidate: &str) -> bool {
    candidate.starts_with('/') || candidate.starts_with('~')
}

pub fn source_problem(
    candidate: &str,
    errors: &[rhebok::config::check::FieldError],
) -> Option<String> {
    let tail = tail_of(candidate);
    errors
        .iter()
        .find(|e| e.path == "sources" && e.message.contains(tail))
        .map(|e| e.message.replace(REFUSAL_TAIL, ""))
}

pub fn source_errors(ctx: &Ctx, item: &str) -> Vec<String> {
    let tail = tail_of(item);
    ctx.errors
        .iter()
        .filter(|e| e.path == "sources" && e.message.contains(tail))
        .map(|e| e.message.clone())
        .collect()
}

pub fn sources_section(
    ctx: &Ctx,
    lists: &HashMap<String, ListState>,
    index: &Entity<IndexStatus>,
    cx: &App,
) -> Vec<AnyElement> {
    let t = *tokens(cx);
    let busy = index.read(cx).busy();
    let sources = list_editor(
        ctx,
        ListEditor {
            path: "sources",
            add_label: "Add folder",
            validating: true,
        },
        lists.get("sources"),
        |item| source_errors(ctx, item),
        cx,
    );
    let ignore = list_editor(
        ctx,
        ListEditor {
            path: "ignore",
            add_label: "Add entry",
            validating: false,
        },
        lists.get("ignore"),
        |_| Vec::new(),
        cx,
    );
    let folders = card("card-folders", "Folders and files", None, cx)
        .child(shell(
            ctx,
            Field::new("sources", "Sources").hint(SOURCES_HINT),
            sources,
            cx,
        ))
        .child(shell(
            ctx,
            Field::new("ignore", "Ignore list").hint(IGNORE_HINT),
            ignore,
            cx,
        ));
    let sessions = card("card-sessions", "Agent sessions", None, cx)
        .child(toggle_field(
            ctx,
            "index_transcripts",
            "Index session transcripts",
            Some(TRANSCRIPTS_HINT.into()),
            cx,
        ))
        .child(toggle_field(
            ctx,
            "index_codex_sessions",
            "Index Codex sessions",
            Some(Hint::Parts(vec![
                Part::Text("Codex's sessions from "),
                Part::Code("$CODEX_HOME/sessions"),
                Part::Text(" ("),
                Part::Code("~/.codex/sessions"),
                Part::Text("). Only counts while session transcripts are on."),
            ])),
            cx,
        ))
        .child(number_field(
            ctx,
            MAX_AGE,
            "Max age in days",
            Some(MAX_AGE_HINT.into()),
            cx,
        ));
    let start = index.downgrade();
    let reindex = button::primary(
        "settings-reindex",
        if busy { "Indexing…" } else { "Re-index now" },
    )
    .disabled(busy)
    .on_click(move |_, _, cx| {
        let _ = start.update(cx, |status, cx| status.start(false, cx));
    })
    .into_any_element();
    let mut parts = vec![
        Part::Text("Runs "),
        Part::Code("rhebok index"),
        Part::Text(
            " in the background with the SAVED settings: only new and changed files are read.",
        ),
    ];
    if !ctx.pending.is_empty() {
        parts.push(Part::Text(UNSAVED));
    }
    let index_card = card("card-index", "Index", Some(reindex), cx)
        .child(
            code_line(&parts, SM_FACE, INDEX_CODE_FACE, Some(t.sunken), cx)
                .id("settings-index-note")
                .test_support()
                .size_sm()
                .text_color(t.muted),
        )
        .when(!matches!(index.read(cx).state(), IndexState::Idle), |d| {
            d.child(index.read(cx).element("settings-index-status".into(), &t))
        });
    vec![
        folders.into_any_element(),
        sessions.into_any_element(),
        index_card.into_any_element(),
    ]
}

#[cfg(test)]
mod tests {
    use rhebok::config::check::FieldError;

    use super::{source_problem, tail_of};

    #[test]
    fn a_source_problem_names_the_candidate_without_the_index_refusal() {
        assert_eq!(tail_of("~/notes//"), "/notes");
        let errors = [FieldError::new(
            "sources",
            "`/Users/x/notes` does not exist, and `rhebok index` refuses to run until it does",
        )];
        assert_eq!(
            source_problem("~/notes/", &errors).as_deref(),
            Some("`/Users/x/notes` does not exist")
        );
        assert_eq!(source_problem("/elsewhere", &errors), None);
    }
}
