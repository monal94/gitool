use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct Highlighter;

impl Highlighter {
    pub fn new() -> Self {
        Self
    }

    /// Highlight a window of diff lines with clear, high-contrast colors.
    /// No syntect — uses simple diff-aware coloring like lazygit/gitui.
    pub fn highlight_diff_window(&self, content: &str, start: usize, count: usize) -> Vec<Line<'static>> {
        content
            .lines()
            .skip(start)
            .take(count)
            .map(|line| colorize_diff_line(line))
            .collect()
    }
}

fn colorize_diff_line(line: &str) -> Line<'static> {
    if line.starts_with("diff --git") || line.starts_with("index ") {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }
    if line.starts_with("--- ") || line.starts_with("+++ ") {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Yellow),
        ));
    }
    if line.starts_with("@@") {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Cyan),
        ));
    }
    if let Some(rest) = line.strip_prefix('+') {
        return Line::from(vec![
            Span::styled("+", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(rest.to_string(), Style::default().fg(Color::Green)),
        ]);
    }
    if let Some(rest) = line.strip_prefix('-') {
        return Line::from(vec![
            Span::styled("-", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(rest.to_string(), Style::default().fg(Color::Red)),
        ]);
    }
    // Context line
    Line::from(Span::styled(
        line.to_string(),
        Style::default().fg(Color::White),
    ))
}

/// Extract file extension from a "diff --git a/foo.rs b/foo.rs" line.
fn extract_extension(diff_line: &str) -> Option<String> {
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
    fn colorize_added_line() {
        let line = colorize_diff_line("+fn main() {}");
        assert_eq!(line.spans.len(), 2);
    }

    #[test]
    fn colorize_removed_line() {
        let line = colorize_diff_line("-old code");
        assert_eq!(line.spans.len(), 2);
    }

    #[test]
    fn colorize_header_line() {
        let line = colorize_diff_line("diff --git a/test.rs b/test.rs");
        assert_eq!(line.spans.len(), 1);
    }

    #[test]
    fn colorize_hunk_header() {
        let line = colorize_diff_line("@@ -1,3 +1,4 @@ fn main()");
        assert_eq!(line.spans.len(), 1);
    }

    #[test]
    fn colorize_context_line() {
        let line = colorize_diff_line(" unchanged code");
        assert_eq!(line.spans.len(), 1);
    }

    #[test]
    fn window_respects_start_and_count() {
        let content = "line0\nline1\nline2\nline3\nline4";
        let h = Highlighter::new();
        let result = h.highlight_diff_window(content, 1, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn window_empty_input() {
        let h = Highlighter::new();
        let result = h.highlight_diff_window("", 0, 10);
        assert!(result.is_empty());
    }
}
