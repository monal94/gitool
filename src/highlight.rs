use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{self, ThemeSet};
use syntect::parsing::SyntaxSet;

pub struct Highlighter {
    ps: SyntaxSet,
    ts: ThemeSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            ps: SyntaxSet::load_defaults_newlines(),
            ts: ThemeSet::load_defaults(),
        }
    }

    /// Highlight diff content with syntax-aware coloring.
    /// Detects language from diff headers and applies syntax highlighting
    /// within added/removed/context lines. Diff markers (+/-/@@) get
    /// their own coloring overlaid.
    #[allow(dead_code)]
    pub fn highlight_diff(&self, content: &str) -> Vec<Line<'static>> {
        let theme = &self.ts.themes["base16-ocean.dark"];
        let mut current_syntax = self.ps.find_syntax_plain_text();
        let mut highlighter = HighlightLines::new(current_syntax, theme);

        content
            .lines()
            .map(|line| {
                // Detect file boundaries and switch syntax
                if line.starts_with("diff --git") {
                    if let Some(ext) = extract_extension(line)
                        && let Some(syntax) = self.ps.find_syntax_by_extension(&ext) {
                            current_syntax = syntax;
                            highlighter = HighlightLines::new(current_syntax, theme);
                        }
                    return make_header_line(line);
                }

                if line.starts_with("index ") || line.starts_with("---") || line.starts_with("+++") {
                    return make_header_line(line);
                }

                if line.starts_with("@@") {
                    return Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::Cyan),
                    ));
                }

                // For +/- lines, strip the marker, highlight the code, then re-add marker styling
                let (marker, code, base_fg) = if let Some(rest) = line.strip_prefix('+') {
                    ("+", rest, Color::Green)
                } else if let Some(rest) = line.strip_prefix('-') {
                    ("-", rest, Color::Red)
                } else {
                    // Context line — highlight normally
                    let spans = syntect_to_spans(&mut highlighter, line, &self.ps);
                    if spans.is_empty() {
                        return Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    return Line::from(spans);
                };

                // Highlight the code portion
                let code_spans = syntect_to_spans(&mut highlighter, code, &self.ps);

                let mut spans = vec![Span::styled(
                    marker.to_string(),
                    Style::default().fg(base_fg).add_modifier(Modifier::BOLD),
                )];

                if code_spans.is_empty() {
                    spans.push(Span::styled(
                        code.to_string(),
                        Style::default().fg(base_fg),
                    ));
                } else {
                    // Merge syntax colors with diff base color
                    for span in code_spans {
                        let fg = match span.style.fg {
                            Some(Color::Reset) | Some(Color::DarkGray) | None => base_fg,
                            Some(c) => c,
                        };
                        spans.push(Span::styled(
                            span.content.into_owned(),
                            Style::default().fg(fg),
                        ));
                    }
                }

                Line::from(spans)
            })
            .collect()
    }

    /// Highlight only a window of lines from the diff for rendering.
    /// This avoids processing 100k lines when only 50 are visible.
    pub fn highlight_diff_window(&self, content: &str, start: usize, count: usize) -> Vec<Line<'static>> {
        let theme = &self.ts.themes["base16-ocean.dark"];
        let mut current_syntax = self.ps.find_syntax_plain_text();
        let mut highlighter = HighlightLines::new(current_syntax, theme);
        let end = start + count;

        content
            .lines()
            .enumerate()
            .map(|(i, line)| {
                // Track syntax changes even for lines before the window
                if line.starts_with("diff --git") {
                    if let Some(ext) = extract_extension(line)
                        && let Some(syntax) = self.ps.find_syntax_by_extension(&ext) {
                            current_syntax = syntax;
                            highlighter = HighlightLines::new(current_syntax, theme);
                        }
                }

                // Only fully process lines in the visible window
                if i < start || i >= end {
                    return Line::from(""); // placeholder for out-of-window lines
                }

                if line.starts_with("diff --git") || line.starts_with("index ") || line.starts_with("---") || line.starts_with("+++") {
                    return make_header_line(line);
                }
                if line.starts_with("@@") {
                    return Line::from(Span::styled(line.to_string(), Style::default().fg(Color::Cyan)));
                }

                let (marker, code, base_fg) = if let Some(rest) = line.strip_prefix('+') {
                    ("+", rest, Color::Green)
                } else if let Some(rest) = line.strip_prefix('-') {
                    ("-", rest, Color::Red)
                } else {
                    let spans = syntect_to_spans(&mut highlighter, line, &self.ps);
                    if spans.is_empty() {
                        return Line::from(Span::styled(line.to_string(), Style::default().fg(Color::DarkGray)));
                    }
                    return Line::from(spans);
                };

                let code_spans = syntect_to_spans(&mut highlighter, code, &self.ps);
                let mut spans = vec![Span::styled(marker.to_string(), Style::default().fg(base_fg).add_modifier(Modifier::BOLD))];
                if code_spans.is_empty() {
                    spans.push(Span::styled(code.to_string(), Style::default().fg(base_fg)));
                } else {
                    for span in code_spans {
                        let fg = match span.style.fg {
                            Some(Color::Reset) | Some(Color::DarkGray) | None => base_fg,
                            Some(c) => c,
                        };
                        spans.push(Span::styled(span.content.into_owned(), Style::default().fg(fg)));
                    }
                }
                Line::from(spans)
            })
            .skip(start)
            .take(count)
            .collect()
    }
}

fn make_header_line(line: &str) -> Line<'static> {
    Line::from(Span::styled(
        line.to_string(),
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    ))
}

/// Convert syntect highlighted ranges to ratatui Spans.
fn syntect_to_spans<'a>(h: &mut HighlightLines, line: &str, ps: &SyntaxSet) -> Vec<Span<'a>> {
    let ranges = h.highlight_line(line, ps);
    let Ok(ranges) = ranges else { return Vec::new() };

    ranges
        .iter()
        .map(|(style, text)| {
            let fg = syntect_color_to_ratatui(style.foreground);
            Span::styled(
                text.to_string(),
                Style::default().fg(fg),
            )
        })
        .collect()
}

fn syntect_color_to_ratatui(c: highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Extract file extension from a "diff --git a/foo.rs b/foo.rs" line.
fn extract_extension(diff_line: &str) -> Option<String> {
    // Parse "diff --git a/path/to/file.ext b/path/to/file.ext"
    let parts: Vec<&str> = diff_line.split_whitespace().collect();
    let path = parts.last()?;
    let path = path.strip_prefix("b/")?;
    let dot_pos = path.rfind('.')?;
    Some(path[dot_pos + 1..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_extension_rust() {
        let line = "diff --git a/src/main.rs b/src/main.rs";
        assert_eq!(extract_extension(line), Some("rs".to_string()));
    }

    #[test]
    fn extract_extension_typescript() {
        let line = "diff --git a/src/app.tsx b/src/app.tsx";
        assert_eq!(extract_extension(line), Some("tsx".to_string()));
    }

    #[test]
    fn extract_extension_no_extension() {
        let line = "diff --git a/Makefile b/Makefile";
        assert_eq!(extract_extension(line), None);
    }

    #[test]
    fn highlight_diff_produces_lines() {
        let h = Highlighter::new();
        let diff = "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,3 +1,3 @@\n-fn old() {}\n+fn new() {}\n context line\n";
        let lines = h.highlight_diff(diff);
        assert_eq!(lines.len(), 7);
    }

    #[test]
    fn highlight_diff_empty_input() {
        let h = Highlighter::new();
        let lines = h.highlight_diff("");
        assert!(lines.is_empty());
    }
}
