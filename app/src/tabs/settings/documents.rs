use gpui_kit::*;

use super::fields::{card, grid, number_field, select_field, text_field, Ctx, Field, InputSpec};

pub const OCR_HINT: &str = "auto recognises only pages that have no text layer.";

pub const PATHS: [(&str, &str); 3] = [
    ("pdf.model_dir", "OCR model folder"),
    ("pdf.pdfium_lib_path", "PDFium library"),
    ("pdf.ort_dylib_path", "ONNX Runtime library"),
];

pub fn inputs() -> Vec<InputSpec> {
    let mut out = vec![
        InputSpec::number("pdf.ocr_min_confidence", false, None),
        InputSpec::number("pdf.dpi", false, None),
    ];
    out.extend(
        PATHS
            .iter()
            .map(|(path, _)| InputSpec::text(path, true, None)),
    );
    out
}

pub fn documents_section(ctx: &Ctx, cx: &App) -> Vec<AnyElement> {
    let pdf = card("card-pdf", "PDF and OCR", None, cx)
        .child(select_field(
            ctx,
            Field::new("pdf.ocr", "OCR").hint(OCR_HINT),
            &["auto", "off", "force"],
            None,
            cx,
        ))
        .child(grid(vec![
            number_field(
                ctx,
                "pdf.ocr_min_confidence",
                "Min OCR confidence (0-1)",
                None,
                cx,
            ),
            number_field(ctx, "pdf.dpi", "Render DPI", None, cx),
        ]))
        .child(select_field(
            ctx,
            Field::new("pdf.model_downloads", "OCR model downloads"),
            &["if-missing", "offline"],
            None,
            cx,
        ))
        .children(
            PATHS
                .iter()
                .map(|(path, label)| text_field(ctx, Field::new(path, label), true, cx)),
        );
    vec![pdf.into_any_element()]
}
