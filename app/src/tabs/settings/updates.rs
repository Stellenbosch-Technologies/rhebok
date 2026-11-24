use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::fields::{
    bounds_recorder, card, error_list, grid, label, number_field, shell, text_field, toggle_field,
    Ctx, Field, InputSpec,
};
use super::list_editor::string_list;
use crate::theme::{tokens, Tokens, Type};
use crate::widgets::checkbox::checkbox;
use crate::widgets::exact_text;
use crate::widgets::wrap::wrapped_text;

pub const SM_LEADING: f32 = 1.25 / 0.875;
pub const BESIDE_DB: &str = "beside the database";
pub const TARGETS: [(&str, &str); 2] = [("s3", "Amazon S3"), ("drive", "Google Drive")];
pub const S3_FIELDS: [(&str, &str); 5] = [
    ("backup.s3.bucket", "Bucket"),
    ("backup.s3.region", "Region"),
    ("backup.s3.prefix", "Prefix"),
    ("backup.s3.profile", "AWS profile"),
    ("backup.s3.storage_class", "Storage class"),
];

pub fn inputs() -> Vec<InputSpec> {
    let mut out = vec![
        InputSpec::number("backup.keep_generations", false, None),
        InputSpec::number("backup.keep_index", false, None),
        InputSpec::text("backup.key_file", true, Some(BESIDE_DB)),
        InputSpec::text("backup.drive.folder_id", false, None),
        InputSpec::text("backup.drive.client_secret_file", false, None),
        InputSpec::text("backup.drive.token_file", true, Some(BESIDE_DB)),
    ];
    out.extend(
        S3_FIELDS
            .iter()
            .map(|(path, _)| InputSpec::text(path, false, None)),
    );
    out
}

pub fn flipped(targets: &[String], target: &str, on: bool) -> Vec<String> {
    let mut out: Vec<String> = targets.iter().filter(|x| *x != target).cloned().collect();
    if on {
        out.push(target.to_string());
    }
    out
}

fn targets_field(ctx: &Ctx, cx: &App) -> AnyElement {
    let targets = string_list(ctx.value("backup.targets").as_ref());
    let control = div()
        .flex()
        .gap(px(16.))
        .children(TARGETS.iter().map(|(target, name)| {
            let on = targets.contains(&target.to_string());
            let next = flipped(&targets, target, !on);
            let set = ctx.setter(
                "backup.targets",
                Some(toml::Value::Array(
                    next.into_iter().map(toml::Value::String).collect(),
                )),
            );
            div()
                .id(SharedString::from(format!("backup-target-{target}-label")))
                .flex()
                .items_center()
                .gap(px(6.))
                .size_sm()
                .when(!ctx.locked, |d| d.on_click(move |_, w, cx| set(w, cx)))
                .child(checkbox(format!("backup-target-{target}"), *name, on).disabled(ctx.locked))
                .child(exact_text(*name))
        }))
        .into_any_element();
    shell(ctx, Field::new("backup.targets", "Targets"), control, cx)
}

fn secret_state(id: &str, name: &str, present: bool, hint: &str, t: &Tokens) -> AnyElement {
    let state = if present { "present" } else { "not present" };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .size_sm()
        .child(
            label(name)
                .line_height(px(11. * SM_LEADING))
                .text_color(t.muted),
        )
        .child(
            div()
                .id(SharedString::from(format!("secret-{id}")))
                .test_support()
                .aria_label(state)
                .child(
                    exact_text(state)
                        .semibold()
                        .text_size(px(11.))
                        .line_height(px(11. * SM_LEADING))
                        .text_color(if present { t.accent } else { t.muted }),
                ),
        )
        .child(
            div()
                .min_w(px(0.))
                .size_xs()
                .text_color(t.muted)
                .child(wrapped_text(hint.to_string())),
        )
        .into_any_element()
}

fn exact_errors(ctx: &Ctx, path: &str, t: &Tokens, cx: &App) -> Option<AnyElement> {
    let errors: Vec<_> = ctx.errors.iter().filter(|e| e.path == path).collect();
    error_list(path, &errors, ctx.blocking, t, cx)
}

pub fn updates_section(ctx: &Ctx, cx: &App) -> Vec<AnyElement> {
    let t = *tokens(cx);
    let updates = card("card-updates", "Updates", None, cx).child(toggle_field(
        ctx,
        "update.check",
        "Check for new releases",
        Some("Checked in the background at session start.".into()),
        cx,
    ));
    let backup = card("card-backup", "Backup", None, cx)
        .child(toggle_field(ctx, "backup.enabled", "Enabled", None, cx))
        .child(targets_field(ctx, cx))
        .child(grid(vec![
            toggle_field(ctx, "backup.include_index", "Include the index", None, cx),
            toggle_field(ctx, "backup.encrypt", "Encrypt", None, cx),
            number_field(ctx, "backup.keep_generations", "Keep generations", None, cx),
            number_field(ctx, "backup.keep_index", "Keep index copies", None, cx),
        ]))
        .child(text_field(
            ctx,
            Field::new("backup.key_file", "Key file"),
            true,
            cx,
        ))
        .child(secret_state(
            "backup.key",
            "Encryption key",
            ctx.state.secret_set("backup.key"),
            "Created by rhebok backup; never shown here.",
            &t,
        ));
    let s3 = card("card-s3", "Amazon S3", None, cx)
        .children(exact_errors(ctx, "backup.s3", &t, cx))
        .child(grid(
            S3_FIELDS
                .iter()
                .map(|(path, name)| text_field(ctx, Field::new(path, name), true, cx))
                .collect(),
        ));
    let drive = card("card-drive", "Google Drive", None, cx)
        .children(exact_errors(ctx, "backup.drive", &t, cx))
        .child(text_field(
            ctx,
            Field::new("backup.drive.folder_id", "Folder id"),
            true,
            cx,
        ))
        .child(text_field(
            ctx,
            Field::new("backup.drive.client_secret_file", "Client secret file"),
            true,
            cx,
        ))
        .child(text_field(
            ctx,
            Field::new("backup.drive.token_file", "Token file"),
            true,
            cx,
        ))
        .child(secret_state(
            "backup.drive_token",
            "Drive token",
            ctx.state.secret_set("backup.drive_token"),
            "Written by rhebok backup auth; never shown here.",
            &t,
        ));
    let anchored = |path: &str, body: Div| {
        div()
            .relative()
            .child(bounds_recorder(ctx, path))
            .child(body)
            .into_any_element()
    };
    vec![
        updates.into_any_element(),
        backup.into_any_element(),
        anchored("backup.s3", s3),
        anchored("backup.drive", drive),
    ]
}

#[cfg(test)]
mod tests {
    use super::flipped;

    #[test]
    fn a_ticked_target_moves_to_the_end_of_the_list() {
        let list = vec!["s3".to_string(), "drive".to_string()];
        assert_eq!(flipped(&list, "s3", true), ["drive", "s3"]);
        assert_eq!(flipped(&list, "s3", false), ["drive"]);
    }
}
