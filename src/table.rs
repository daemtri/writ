//! Markdown pipe table detection and formatting.
//!
//! v1: aligned columns only. Rendering is line-based: we replace the entire
//! line's displayed text (via StyledRegion.display_text) when the cursor is not
//! inside the table block.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableBlock {
    pub start_line: usize,
    pub end_line_exclusive: usize,
    pub columns: usize,
    pub align: Vec<ColumnAlign>,
    pub widths: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRender {
    pub block: TableBlock,
    /// Per-line replacement for the full line range.
    pub replacements: Vec<(usize, Range<usize>, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRow {
    line_idx: usize,
    line_range: Range<usize>,
    cells: Vec<String>,
}

/// Detect pipe-table blocks in the provided lines.
///
/// `lines` are tuples of (line_idx, full_line_range_in_bytes, line_text).
/// `line_text` should not include the trailing newline.
pub fn detect_pipe_tables(lines: &[(usize, Range<usize>, String)]) -> Vec<TableRender> {
    let mut out = Vec::new();

    let mut i = 0;
    while i + 1 < lines.len() {
        // header row candidate
        let (h_idx, h_range, h_text) = &lines[i];
        if !is_pipe_table_row_candidate(h_text) {
            i += 1;
            continue;
        }

        // delimiter row candidate
        let (d_idx, d_range, d_text) = &lines[i + 1];
        if !is_delimiter_row(d_text) {
            i += 1;
            continue;
        }

        let header_cells = split_pipe_row(h_text);
        let (align, delim_cols) = parse_delimiter_alignment(d_text);

        // Require a sensible column count.
        let columns = header_cells.len().max(delim_cols);
        if columns == 0 {
            i += 1;
            continue;
        }

        // Table blocks require alignment row to match column count roughly.
        // We allow off-by-one formatting quirks by taking max.
        let align = normalize_align(align, columns);

        // Parse body rows (and also include header in width calc).
        let mut rows: Vec<ParsedRow> = Vec::new();
        rows.push(ParsedRow {
            line_idx: *h_idx,
            line_range: h_range.clone(),
            cells: normalize_cells(header_cells, columns),
        });

        // We keep delimiter row for extent but it won't be rendered in pretty form.
        let delim_line_idx = *d_idx;
        let delim_line_range = d_range.clone();

        let mut end = i + 2;
        while end < lines.len() {
            let (line_idx, line_range, text) = &lines[end];
            if !is_pipe_table_row_candidate(text) {
                break;
            }
            let cells = normalize_cells(split_pipe_row(text), columns);
            rows.push(ParsedRow {
                line_idx: *line_idx,
                line_range: line_range.clone(),
                cells,
            });
            end += 1;
        }

        let widths = compute_widths(&rows, columns);

        let block = TableBlock {
            start_line: *h_idx,
            end_line_exclusive: lines[end.saturating_sub(1)].0 + 1,
            columns,
            align,
            widths,
        };

        let mut replacements: Vec<(usize, Range<usize>, String)> = Vec::new();

        // Header + body rows.
        for row in &rows {
            let rendered = format_row(&row.cells, &block.widths, &block.align);
            replacements.push((row.line_idx, row.line_range.clone(), rendered));
        }

        // Delimiter row: render as empty (keep line height stable by using a single space).
        replacements.push((delim_line_idx, delim_line_range, " ".to_string()));

        out.push(TableRender {
            block,
            replacements,
        });

        // Advance.
        i = end;
    }

    out
}

fn is_pipe_table_row_candidate(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    // Must contain a pipe and not be a fenced code line.
    if !t.contains('|') {
        return false;
    }
    // Avoid treating markdown code blocks as tables.
    if t.starts_with("```") || t.starts_with("~~~") {
        return false;
    }
    true
}

fn is_delimiter_row(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || !t.contains('|') {
        return false;
    }
    let cols = split_pipe_row(t);
    if cols.is_empty() {
        return false;
    }
    cols.iter().all(|c| is_delim_cell(c))
}

fn is_delim_cell(cell: &str) -> bool {
    let t = cell.trim();
    if t.is_empty() {
        return false;
    }
    // Allowed: hyphens with optional leading/trailing colon.
    let bytes = t.as_bytes();
    let mut i = 0;
    if bytes[i] == b':' {
        i += 1;
        if i >= bytes.len() {
            return false;
        }
    }
    let mut dash_count = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'-' => {
                dash_count += 1;
                i += 1;
            }
            b':' if i == bytes.len() - 1 => {
                i += 1;
            }
            _ => return false,
        }
    }
    // GitHub allows at least 1 dash; many users write :--: etc.
    dash_count >= 1
}

fn split_pipe_row(s: &str) -> Vec<String> {
    // Basic GitHub-flavored splitting:
    // - ignore leading/trailing pipe
    // - support escaped pipes: \|
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_was_backslash = false;

    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if prev_was_backslash {
            current.push(ch);
            prev_was_backslash = false;
            continue;
        }
        if ch == '\\' {
            prev_was_backslash = true;
            current.push(ch);
            continue;
        }
        if ch == '|' {
            parts.push(current);
            current = String::new();
        } else {
            current.push(ch);
        }
    }
    parts.push(current);

    // Trim outer empty parts caused by leading/trailing pipes.
    while parts.first().is_some_and(|p| p.trim().is_empty()) {
        parts.remove(0);
    }
    while parts.last().is_some_and(|p| p.trim().is_empty()) {
        parts.pop();
    }

    parts.into_iter().map(|p| p.trim().to_string()).collect()
}

fn parse_delimiter_alignment(s: &str) -> (Vec<ColumnAlign>, usize) {
    let cols = split_pipe_row(s);
    let mut align = Vec::new();
    for c in &cols {
        let t = c.trim();
        let left = t.starts_with(':');
        let right = t.ends_with(':');
        let a = match (left, right) {
            (true, true) => ColumnAlign::Center,
            (false, true) => ColumnAlign::Right,
            _ => ColumnAlign::Left,
        };
        align.push(a);
    }
    (align, cols.len())
}

fn normalize_align(mut align: Vec<ColumnAlign>, columns: usize) -> Vec<ColumnAlign> {
    if align.len() < columns {
        align.extend(std::iter::repeat(ColumnAlign::Left).take(columns - align.len()));
    }
    align.truncate(columns);
    align
}

fn normalize_cells(mut cells: Vec<String>, columns: usize) -> Vec<String> {
    if cells.len() < columns {
        cells.extend(std::iter::repeat(String::new()).take(columns - cells.len()));
    }
    cells.truncate(columns);
    cells
}

fn visible_width(s: &str) -> usize {
    // v1: count Unicode scalar values (chars), after trimming.
    s.trim().chars().count()
}

fn compute_widths(rows: &[ParsedRow], columns: usize) -> Vec<usize> {
    let mut widths = vec![0usize; columns];
    for row in rows {
        for (i, cell) in row.cells.iter().enumerate().take(columns) {
            widths[i] = widths[i].max(visible_width(cell));
        }
    }
    widths
}

fn pad_to_width(s: &str, width: usize, align: ColumnAlign) -> String {
    let t = s.trim();
    let len = t.chars().count();
    if len >= width {
        return t.to_string();
    }
    let pad = width - len;
    match align {
        ColumnAlign::Left => format!("{}{}", t, " ".repeat(pad)),
        ColumnAlign::Right => format!("{}{}", " ".repeat(pad), t),
        ColumnAlign::Center => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{}{}", " ".repeat(left), t, " ".repeat(right))
        }
    }
}

fn format_row(cells: &[String], widths: &[usize], align: &[ColumnAlign]) -> String {
    let mut out = String::new();
    for i in 0..cells.len().min(widths.len()).min(align.len()) {
        if i > 0 {
            out.push_str("  ");
        }
        out.push_str(&pad_to_width(&cells[i], widths[i], align[i]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_simple_table_and_formats_rows() {
        let lines = vec![
            (0usize, 0..9, "| a | bb |".to_string()),
            (1usize, 10..23, "| --- | ---: |".to_string()),
            (2usize, 24..36, "| ccc | d |".to_string()),
        ];
        let tables = detect_pipe_tables(&lines);
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.block.columns, 2);
        assert_eq!(t.block.align, vec![ColumnAlign::Left, ColumnAlign::Right]);
        assert_eq!(t.block.widths, vec![3, 2]);

        // Header replacement
        let header = t.replacements.iter().find(|(ix, _, _)| *ix == 0).unwrap();
        assert_eq!(header.2, "a    bb");

        // Body replacement
        let body = t.replacements.iter().find(|(ix, _, _)| *ix == 2).unwrap();
        assert_eq!(body.2, "ccc   d");

        // Delimiter replacement
        let delim = t.replacements.iter().find(|(ix, _, _)| *ix == 1).unwrap();
        assert_eq!(delim.2, " ");
    }

    #[test]
    fn delimiter_with_two_dashes_is_accepted() {
        let lines = vec![
            (0usize, 0..27, "| 姓名 | 年龄 | 城市 |".to_string()),
            (1usize, 28..49, "| :--- | :--: | ---: |".to_string()),
            (2usize, 50..70, "| 张三 | 25   | 北京 |".to_string()),
        ];
        let tables = detect_pipe_tables(&lines);
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.block.columns, 3);
        assert_eq!(
            t.block.align,
            vec![ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right]
        );
    }

    #[test]
    fn escaped_pipes_are_not_split() {
        let parts = split_pipe_row("| a\\|b | c |");
        assert_eq!(parts, vec!["a\\|b".to_string(), "c".to_string()]);
    }
}
