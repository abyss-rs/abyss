use std::path::Path;

use super::{MAX_HIGHLIGHT_LINES, highlight};

fn lines(source: &str) -> Vec<String> {
    source.lines().map(ToOwned::to_owned).collect()
}

#[test]
fn rust_source_is_highlighted_line_for_line() {
    let source = lines("fn main() {\n    let x = 1;\n}");
    let highlighted = highlight(Path::new("main.rs"), &source).expect("rust is a bundled syntax");
    assert_eq!(
        highlighted.len(),
        source.len(),
        "every input line needs a rendered counterpart"
    );
    assert!(
        highlighted.iter().any(|line| line.len() > 1),
        "at least one line should split into differently styled runs"
    );
}

#[test]
fn highlighting_preserves_the_text_exactly() {
    let source = lines("fn main() {\n    let x = 1;\n}");
    let highlighted = highlight(Path::new("main.rs"), &source).expect("rust is a bundled syntax");
    for (original, pieces) in source.iter().zip(&highlighted) {
        let rebuilt: String = pieces.iter().map(|piece| piece.text.as_str()).collect();
        assert_eq!(&rebuilt, original, "highlighting must not alter the text");
    }
}

#[test]
fn keywords_and_plain_text_get_different_colours() {
    let source = lines("fn main() {}");
    let highlighted = highlight(Path::new("main.rs"), &source).expect("rust is a bundled syntax");
    let colours: Vec<_> = highlighted[0].iter().map(|piece| piece.color).collect();
    assert!(
        colours.windows(2).any(|pair| pair[0] != pair[1]),
        "a keyword and an identifier should not share a colour: {colours:?}"
    );
}

#[test]
fn unknown_extensions_fall_back_to_plain_text() {
    let source = lines("some text\nmore text");
    assert!(highlight(Path::new("notes.zzzzz"), &source).is_none());
}

#[test]
fn files_without_an_extension_can_still_match_by_name() {
    let source = lines("FROM rust:1.97\nRUN cargo build");
    // bat's mapping recognises Dockerfile by name where an extension lookup
    // would find nothing.
    assert!(highlight(Path::new("Dockerfile"), &source).is_some());
}

#[test]
fn empty_and_oversized_inputs_are_declined() {
    assert!(highlight(Path::new("main.rs"), &[]).is_none());
    let huge = vec!["fn f() {}".to_owned(); MAX_HIGHLIGHT_LINES + 1];
    assert!(
        highlight(Path::new("main.rs"), &huge).is_none(),
        "a file this long must not stall the viewer"
    );
}

#[test]
fn horizontal_scrolling_slices_runs_without_losing_characters() {
    // Mirrors what the viewer does when scrolled right: the visible text must
    // match a plain-string slice of the same window, styling aside.
    let source = lines("fn main() { let value = 12345; }");
    let highlighted = highlight(Path::new("main.rs"), &source).expect("rust is a bundled syntax");
    let pieces = &highlighted[0];
    let whole: String = pieces.iter().map(|piece| piece.text.as_str()).collect();

    for (skip, width) in [(0, 8), (3, 10), (10, 5), (0, 200), (40, 5)] {
        let expected: String = whole.chars().skip(skip).take(width).collect();
        let mut remaining_skip = skip;
        let mut remaining_width = width;
        let mut actual = String::new();
        for piece in pieces {
            if remaining_width == 0 {
                break;
            }
            let length = piece.text.chars().count();
            if remaining_skip >= length {
                remaining_skip -= length;
                continue;
            }
            let visible: String = piece
                .text
                .chars()
                .skip(remaining_skip)
                .take(remaining_width)
                .collect();
            remaining_skip = 0;
            remaining_width -= visible.chars().count();
            actual.push_str(&visible);
        }
        assert_eq!(actual, expected, "skip {skip}, width {width}");
    }
}

#[test]
fn markdown_and_toml_are_recognised_too() {
    assert!(highlight(Path::new("README.md"), &lines("# Title\n\ntext")).is_some());
    assert!(highlight(Path::new("Cargo.toml"), &lines("[package]\nname = \"x\"")).is_some());
}
