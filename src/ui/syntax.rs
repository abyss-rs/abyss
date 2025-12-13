use tree_sitter::{Parser, Language};
use tree_sitter_highlight::{HighlightConfiguration, Highlighter, HighlightEvent};
use std::sync::OnceLock;
use ratatui::style::{Color, Style};
use ratatui::text::{Span, Line};

// Global configurations for supported languages
static RUST_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static GO_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static HCL_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static JAVA_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static RUBY_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
// static SQL_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
static MD_CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();

/// Initialize a highlight configuration with standard capture names
fn make_config(language: Language, highlights_query: &str) -> HighlightConfiguration {
    let mut config = HighlightConfiguration::new(
        language,
        "utf-8", // we'll treat strings as such
        highlights_query,
        "",
        "",
    ).expect("Invalid query");
    
    // We strictly use standard names that map to our colors
    config.configure(&[
        "keyword",
        "function", 
        "string",
        "number",
        "type",
        "comment",
        "operator",
        "punctuation",
        "variable",
        "constant",
    ]);
    
    config
}

/// Load configuration for a given extension
fn get_config(extension: &str) -> Option<&'static HighlightConfiguration> {
    match extension {
        "rs" => Some(RUST_CONFIG.get_or_init(|| {
             // Rust usually exports HIGHLIGHTS_QUERY. If not, we'd need a fallback.
             let query = tree_sitter_rust::HIGHLIGHTS_QUERY;
             make_config(tree_sitter_rust::LANGUAGE.into(), query)
        })),
        "go" => Some(GO_CONFIG.get_or_init(|| {
             let query = tree_sitter_go::HIGHLIGHTS_QUERY;
             make_config(tree_sitter_go::LANGUAGE.into(), query)
        })),
        "tf" | "hcl" => Some(HCL_CONFIG.get_or_init(|| {
             // Manual query for HCL as constant might be missing
             let query = r#"
                (attribute (identifier) @variable)
                (block (type) @type)
                (string_lit) @string
                (number) @number
                (comment) @comment
             "#;
             make_config(tree_sitter_hcl::LANGUAGE.into(), query)
        })),
        "java" => Some(JAVA_CONFIG.get_or_init(|| {
             let query = tree_sitter_java::HIGHLIGHTS_QUERY;
             make_config(tree_sitter_java::LANGUAGE.into(), query)
        })),
        "rb" | "ruby" => Some(RUBY_CONFIG.get_or_init(|| {
             let query = tree_sitter_ruby::HIGHLIGHTS_QUERY;
             make_config(tree_sitter_ruby::LANGUAGE.into(), query)
        })),
        /* SQL disabled due to crate version incompatibility
        "sql" => Some(SQL_CONFIG.get_or_init(|| {
             // ...
        })),
        */
        "md" | "markdown" => Some(MD_CONFIG.get_or_init(|| {
             let query = r#"
                (atx_heading) @keyword
                (fenced_code_block) @variable
                (link_destination) @string
                (link_text) @string
                (list_item) @punctuation
             "#;
             // Using language() for MD as it's older/simpler usually, 
             // Note: tree-sitter-md might expose `language()` or `LANGUAGE`.
             // 0.3 often uses `language()`.
             make_config(tree_sitter_md::LANGUAGE.into(), query) 
        })),
        _ => None,
    }
}

/// Map highlighter capture index to Ratatui Style using standard terminal colors
fn theme_color(highlight_idx: usize) -> Style {
    let style = Style::default();
    match highlight_idx {
        0 => style.fg(Color::Magenta), // keyword
        1 => style.fg(Color::Blue),    // function
        2 => style.fg(Color::Green),   // string
        3 => style.fg(Color::Yellow),  // number (using yellow for visibility)
        4 => style.fg(Color::Cyan),    // type
        5 => style.fg(Color::DarkGray),// comment
        6 => style.fg(Color::White),   // operator
        7 => style.fg(Color::White),   // punctuation
        8 => style.fg(Color::White),   // variable (default text)
        9 => style.fg(Color::Red),     // constant
        _ => style.fg(Color::White),
    }
}

/// Highlight content and return visible lines
pub fn highlight_content(content: &str, extension: &str) -> Vec<Line<'static>> {
    let config = match get_config(extension) {
        Some(c) => c,
        None => {
            // Fallback: simple plain text lines
            return content.lines()
                .map(|l| Line::from(vec![Span::styled(l.to_string(), Style::default().fg(Color::White))]))
                .collect();
        }
    };

    let mut highlighter = Highlighter::new();
    // Safety check: ensure content is not empty to avoid parser issues
    if content.is_empty() {
        return vec![Line::from("")];
    }

    let events = match highlighter.highlight(config, content.as_bytes(), None, |_| None) {
        Ok(e) => e,
        Err(_) => {
            // Fallback on error
             return content.lines()
                .map(|l| Line::from(vec![Span::styled(l.to_string(), Style::default().fg(Color::White))]))
                .collect();
        }
    };
    
    let mut lines = Vec::new();
    let mut current_spans = Vec::new();
    let mut current_style = Style::default().fg(Color::White); // Default
    
    // let mut text_buffer = String::new(); // Unused

    for event in events {
        match event {
            Ok(HighlightEvent::Source { start, end }) => {
                let text_chunk = &content[start..end];
                
                // Handle newlines explicitly to break spans into lines
                for (_i, part) in text_chunk.split_inclusive('\n').enumerate() {
                    let part_str = part.to_string();
                    let is_newline = part_str.ends_with('\n');
                    let clean_part = if is_newline { &part_str[..part_str.len()-1] } else { &part_str };
                    
                    if !clean_part.is_empty() {
                         current_spans.push(Span::styled(clean_part.to_string(), current_style));
                    }
                    
                    if is_newline {
                        lines.push(Line::from(current_spans.clone()));
                        current_spans.clear();
                    }
                }
            },
            Ok(HighlightEvent::HighlightStart(s)) => {
                current_style = theme_color(s.0);
            },
            Ok(HighlightEvent::HighlightEnd) => {
                current_style = Style::default().fg(Color::White);
            },
            _ => {}
        }
    }
    
    // Push remaining
    if !current_spans.is_empty() {
        lines.push(Line::from(current_spans));
    } else if content.ends_with('\n') {
         // If file ends with newline
         lines.push(Line::from(vec![]));
    }

    lines
}

/// Highlight a single line (limited context)
pub fn highlight_line(line: &str, extension: &str) -> Line<'static> {
    let lines = highlight_content(line, extension);
    lines.into_iter().next().unwrap_or_else(|| Line::from(""))
}
