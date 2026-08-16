#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::OnceLock;

use bat::SyntaxMapping;
use bat::assets::HighlightingAssets;
use ratatui::style::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, Theme};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Files longer than this are shown unhighlighted.
///
/// Syntect parses line by line and cannot resume mid-file, so a long file has
/// to be done in one pass; past this point the pause before the viewer opens
/// is worse than the missing colour.
const MAX_HIGHLIGHT_LINES: usize = 20_000;

/// One run of identically styled text within a line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Piece {
    pub(crate) text: String,
    pub(crate) color: Color,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
}

/// bat's bundled syntaxes and themes.
///
/// Loading them costs a few milliseconds and a few megabytes, so it happens
/// once, on the first file that actually needs highlighting.
struct Assets {
    syntaxes: SyntaxSet,
    theme: Theme,
    mapping: SyntaxMapping<'static>,
}

static ASSETS: OnceLock<Option<Assets>> = OnceLock::new();

fn assets() -> Option<&'static Assets> {
    ASSETS
        .get_or_init(|| {
            let bundled = HighlightingAssets::from_binary();
            let syntaxes = bundled.get_syntax_set().ok()?.clone();
            // A dark default matches the rest of the TUI; bat falls back to a
            // bundled theme rather than failing if the name is unknown.
            let theme = bundled.get_theme("Monokai Extended").clone();
            Some(Assets {
                syntaxes,
                theme,
                mapping: SyntaxMapping::new(),
            })
        })
        .as_ref()
}

/// Highlight `lines` as the language implied by `path`.
///
/// Returns `None` when the file is too long, the language is unrecognised, or
/// the assets could not be loaded — in every case the caller falls back to
/// rendering the plain text it already has.
pub(crate) fn highlight(path: &Path, lines: &[String]) -> Option<Vec<Vec<Piece>>> {
    if lines.is_empty() || lines.len() > MAX_HIGHLIGHT_LINES {
        return None;
    }
    let assets = assets()?;
    let syntax = assets
        .syntaxes
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| {
            // bat's mapping knows names syntect's extension lookup does not,
            // such as `Dockerfile` or `.zshrc`.
            let bundled = HighlightingAssets::from_binary();
            bundled
                .get_syntax_for_path(path, &assets.mapping)
                .ok()
                .map(|found| found.syntax)
                .and_then(|found| assets.syntaxes.find_syntax_by_name(&found.name))
        })?;

    let mut highlighter = HighlightLines::new(syntax, &assets.theme);
    let mut result = Vec::with_capacity(lines.len());
    // Syntect needs the newline to close constructs such as line comments.
    let joined = lines.join("\n") + "\n";
    for line in LinesWithEndings::from(&joined) {
        let Ok(styled) = highlighter.highlight_line(line, &assets.syntaxes) else {
            return None;
        };
        result.push(to_pieces(&styled));
        if result.len() == lines.len() {
            break;
        }
    }
    (result.len() == lines.len()).then_some(result)
}

fn to_pieces(styled: &[(SyntectStyle, &str)]) -> Vec<Piece> {
    styled
        .iter()
        .filter_map(|(style, text)| {
            let text = text.trim_end_matches(['\n', '\r']);
            if text.is_empty() {
                return None;
            }
            Some(Piece {
                text: text.to_owned(),
                color: Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
            })
        })
        .collect()
}
