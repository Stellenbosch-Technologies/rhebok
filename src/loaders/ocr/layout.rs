use super::engine::OcrLine;
use std::collections::HashSet;

pub fn page_text(lines: &[OcrLine], page_width: f32) -> String {
    let mut ordered: Vec<&OcrLine> = lines.iter().collect();
    ordered.sort_by(|a, b| a.top.total_cmp(&b.top).then(a.left.total_cmp(&b.left)));
    let Some(gutter) = gutter(&ordered, page_width) else {
        return rows(&ordered);
    };
    let mut blocks = Vec::new();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for line in ordered {
        if line.right <= gutter {
            left.push(line);
        } else if line.left >= gutter {
            right.push(line);
        } else {
            blocks.push(rows(&std::mem::take(&mut left)));
            blocks.push(rows(&std::mem::take(&mut right)));
            blocks.push(line.text.clone());
        }
    }
    blocks.push(rows(&left));
    blocks.push(rows(&right));
    blocks.retain(|block| !block.is_empty());
    blocks.join("\n")
}

fn gutter(lines: &[&OcrLine], page_width: f32) -> Option<f32> {
    if lines.len() < 6 || page_width <= 0.0 {
        return None;
    }
    let side = lines.len().div_ceil(5);
    (25..=75)
        .map(|percent| page_width * percent as f32 / 100.0)
        .map(|x| {
            let crossing = lines.iter().filter(|l| l.left < x && l.right > x).count();
            let left = lines.iter().filter(|l| l.right <= x).count();
            let right = lines.iter().filter(|l| l.left >= x).count();
            (x, crossing, left.min(right))
        })
        .filter(|&(_, crossing, smaller)| crossing * 5 <= lines.len() && smaller >= side)
        .min_by_key(|&(_, crossing, smaller)| (crossing, usize::MAX - smaller))
        .map(|(x, _, _)| x)
}

fn rows(lines: &[&OcrLine]) -> String {
    let mut rows: Vec<Vec<&OcrLine>> = Vec::new();
    for &line in lines {
        match rows.last_mut() {
            Some(row) if same_row(row[0], line) => row.push(line),
            _ => rows.push(vec![line]),
        }
    }
    rows.into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.left.total_cmp(&b.left));
            row.iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn same_row(a: &OcrLine, b: &OcrLine) -> bool {
    let overlap = a.bottom.min(b.bottom) - a.top.max(b.top);
    let shorter = (a.bottom - a.top).min(b.bottom - b.top);
    shorter > 0.0 && overlap * 2.0 > shorter
}

pub fn with_unseen_lines(native: &str, ocr_text: &str) -> Option<String> {
    let seen: HashSet<String> = words(native).collect();
    let unseen: Vec<&str> = ocr_text
        .lines()
        .filter(|line| {
            let all: Vec<String> = words(line).collect();
            let new = all.iter().filter(|word| !seen.contains(*word)).count();
            !all.is_empty() && new * 2 > all.len()
        })
        .collect();
    (!unseen.is_empty()).then(|| format!("{}\n\n{}", native.trim_end(), unseen.join("\n")))
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, left: f32, top: f32, right: f32) -> OcrLine {
        OcrLine {
            text: text.into(),
            confidence: 1.0,
            left,
            top,
            right,
            bottom: top + 10.0,
        }
    }

    #[test]
    fn pieces_of_one_row_are_joined_left_to_right_and_rows_run_top_to_bottom() {
        let lines = [
            at("second", 0.0, 30.0, 50.0),
            at("world", 60.0, 11.0, 100.0),
            at("hello", 0.0, 10.0, 50.0),
        ];
        assert_eq!(page_text(&lines, 100.0), "hello world\nsecond");
    }

    #[test]
    fn a_two_column_page_reads_the_left_column_before_the_right() {
        let mut lines = vec![at("Title across the page", 0.0, 0.0, 100.0)];
        for row in 0..4 {
            let top = 20.0 + row as f32 * 20.0;
            lines.push(at(&format!("left {row}"), 0.0, top, 45.0));
            lines.push(at(&format!("right {row}"), 55.0, top, 100.0));
        }
        assert_eq!(
            page_text(&lines, 100.0),
            "Title across the page\nleft 0\nleft 1\nleft 2\nleft 3\nright 0\nright 1\nright 2\nright 3"
        );
    }

    #[test]
    fn a_single_column_page_is_not_split() {
        let lines: Vec<OcrLine> = (0..8)
            .map(|row| at(&format!("line {row}"), 0.0, row as f32 * 20.0, 90.0))
            .collect();
        assert_eq!(
            page_text(&lines, 100.0),
            (0..8)
                .map(|r| format!("line {r}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn only_lines_mostly_missing_from_the_text_layer_are_added() {
        let native = "# Results\n\nThe model reached high accuracy.";
        let ocr = "The model reached high accuracy.\nFigure 3: loss curve over epochs\nResults";
        assert_eq!(
            with_unseen_lines(native, ocr).as_deref(),
            Some(
                "# Results\n\nThe model reached high accuracy.\n\nFigure 3: loss curve over epochs"
            )
        );
        assert_eq!(
            with_unseen_lines(native, "The model reached high accuracy."),
            None
        );
    }
}
