use gpui_kit::*;

use super::fields::{card, grid, number_field, text_field, toggle_field, Ctx, Field, InputSpec};

pub const NUMBERS: [(&str, &str); 6] = [
    ("memory.lessons_max_tokens", "Lessons token budget"),
    ("memory.min_confidence", "Min confidence (0-100)"),
    ("memory.duplicate_similarity", "Duplicate similarity (0-1)"),
    ("memory.max_memories", "Max memories"),
    ("memory.episode_half_life_days", "Episode half-life (days)"),
    ("memory.episode_decay_floor", "Episode decay floor (0-1)"),
];

pub const DISTILL_NUMBERS: [(&str, &str); 2] = [
    ("memory.distill_after_hours", "Distill after (hours)"),
    ("memory.distill_idle_secs", "Idle before distilling (s)"),
];

pub fn inputs() -> Vec<InputSpec> {
    NUMBERS
        .iter()
        .chain(DISTILL_NUMBERS.iter())
        .map(|(path, _)| InputSpec::number(path, false, None))
        .chain([InputSpec::text("memory.distill_model", false, None)])
        .collect()
}

fn numbers(ctx: &Ctx, fields: &[(&str, &str)], cx: &App) -> Div {
    grid(
        fields
            .iter()
            .map(|(path, label)| number_field(ctx, path, label, None, cx))
            .collect(),
    )
}

pub fn memory_section(ctx: &Ctx, cx: &App) -> Vec<AnyElement> {
    let memories = card("card-memories", "Memories", None, cx)
        .child(toggle_field(ctx, "memory.enabled", "Enabled", None, cx))
        .child(numbers(ctx, &NUMBERS, cx));
    let distillation = card("card-distillation", "Session distillation", None, cx)
        .child(toggle_field(
            ctx,
            "memory.distill_episodes",
            "Distill sessions into episodes",
            None,
            cx,
        ))
        .child(numbers(ctx, &DISTILL_NUMBERS, cx))
        .child(text_field(
            ctx,
            Field::new("memory.distill_model", "Distillation model"),
            true,
            cx,
        ));
    vec![memories.into_any_element(), distillation.into_any_element()]
}
