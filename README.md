# writ

[![CI](https://github.com/wilfreddenton/writ/actions/workflows/ci.yml/badge.svg)](https://github.com/wilfreddenton/writ/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/writ.svg)](https://crates.io/crates/writ)
[![docs.rs](https://docs.rs/writ/badge.svg)](https://docs.rs/writ)

A hybrid markdown editor combining raw text editing with live inline rendering.

## Install

```bash
cargo install writ --locked
```

## Usage

```bash
writ --file path/to/document.md
```

To try writ without a file, use demo mode which opens and "plays" scripted input:

```bash
writ --demo
```

Fonts can be configured via command line arguments or environment variables:

```bash
writ --file doc.md --text-font "Iosevka Aile" --code-font "Iosevka"
```

```bash
WRIT_TEXT_FONT="Iosevka Aile" WRIT_CODE_FONT="Iosevka" writ --file doc.md
```

The default fonts are platform-specific: Segoe UI and Consolas on Windows, the system font and Menlo on macOS, and Liberation Sans and Liberation Mono on Linux.

### GhostText Integration

writ can edit browser textareas via the [GhostText](https://ghosttext.fregante.com/) protocol. Install the GhostText browser extension, then run the `writd` daemon:

```bash
writd
```

The daemon listens on port 4001. When you activate GhostText on a textarea, `writd` spawns a writ instance with `--autosave` enabled. Edits in writ sync back to the browser in real-time. This is useful for writing GitHub comments, issues, and PRs with writ's markdown editing features.

The `--autosave` flag can also be used standalone to save on every edit:

```bash
writ --file doc.md --autosave
```

## Development

```bash
git clone https://github.com/wilfreddenton/writ
cd writ
cargo run -- --file path/to/document.md
```

On Linux, using a faster linker significantly improves build times. See [Zed's linker documentation](https://github.com/zed-industries/zed/blob/main/docs/src/development/linux.md#linkers-linker) for setup instructions.

### Build Profiles

Debug builds are significantly slower, especially for image loading and text rendering. For day-to-day development with better performance, use the `release-fast` profile:

```bash
cargo run --profile release-fast -- --file path/to/document.md
```

For maximum runtime performance (slower compile times), use a full release build:

```bash
cargo run --release -- --file path/to/document.md
```

The release profile enables thin LTO and single codegen unit for best optimization. The `release-fast` profile trades some runtime performance for faster compilation by disabling LTO and using parallel codegen units.

## Features

### Inline Rendering

Markdown syntax is hidden when your cursor is elsewhere, revealing clean formatted text. Move your cursor to any formatted element and the raw syntax appears for editing. Headings hide their `#` markers and display at the appropriate size. Bold and italic text hides the `*` markers. Inline code hides the backticks and renders in a monospace font. Links hide the URL syntax entirely and can be opened with Ctrl+click (Cmd+click on macOS).

### Images

Images render inline, supporting both URLs and local file paths (absolute or relative to the markdown file). When an image is on its own line, only the rendered image is shown. Move your cursor to the line to reveal the markdown syntax above the image.

### Lists and Blockquotes

Unordered list markers (`-`) are replaced with bullet symbols when the cursor is away. Ordered lists are automatically renumbered as you edit. Task lists render interactive checkboxes that you can click to toggle. Blockquotes hide their `>` markers and show a left border instead.

Nesting is fully supported. A task item inside a blockquote is represented internally as a stack of layers, and each layer contributes its visual treatment independently.

### Smart Enter and Tab

Enter inserts a raw newline—no magic. Shift+Enter continues the current container by copying markers from the current line (e.g., on `- item|`, Shift+Enter creates `\n- `). Shift+Alt+Enter creates an indented continuation for nested paragraphs within list items.

Tab cycles through nesting states based on tree-sitter context. On a blank line after a list item, Tab cycles through: sibling marker → paragraph indent → nested marker → empty. Shift+Tab cycles backwards. This replaces traditional indent/dedent with context-aware structure cycling.

### Code Blocks

Fenced code blocks render with syntax highlighting (currently Rust). The fence lines are hidden when the cursor is outside the block, showing only the highlighted code. Move your cursor into the block to reveal the fences for editing.

### Selection and Editing

Full selection support with click, drag, shift+arrow keys, double-click to select word, and triple-click to select line. Copy, cut, and paste work as expected. Undo and redo are supported with full cursor position restoration.

## Library Usage

writ can be embedded as a GPUI component in your own application. Add it as a dependency:

```bash
cargo add writ
```

### Basic Usage

```rust
use gpui::{prelude::*, Rems};
use writ::{Editor, EditorConfig, EditorTheme};

// Create with default configuration
let editor = cx.new(|cx| Editor::new("# Hello, world!", cx));

// Or with custom configuration
let config = EditorConfig {
    theme: EditorTheme::dracula(),
    text_font: "Inter".to_string(),
    code_font: "JetBrains Mono".to_string(),
    base_path: Some("/path/to/markdown/file".into()),
    padding_x: Rems(2.0),  // Horizontal padding
    padding_y: Rems(1.5),  // Vertical padding (scrolls with content)
};
let editor = cx.new(|cx| Editor::with_config("# Hello", config, cx));

// Access content
let text = editor.read(cx).text();
let is_dirty = editor.read(cx).is_dirty();

// Modify content
editor.update(cx, |e, cx| e.insert("new text", cx));
editor.update(cx, |e, cx| e.set_text("replacement", cx));
```

### Streaming Support

For AI chat applications that stream markdown responses token by token:

```rust
// Start streaming (blocks user input, pins cursor to end)
editor.update(cx, |e, cx| e.begin_streaming(cx));

// Append tokens as they arrive
for token in ai_response_stream {
    editor.update(cx, |e, cx| e.append(&token, cx));
}

// End streaming (restores normal editing)
editor.update(cx, |e, cx| e.end_streaming(cx));
```

### Programmatic Actions

Execute editor actions programmatically:

```rust
use writ::{EditorAction, Direction};

editor.update(cx, |e, cx| {
    e.execute(EditorAction::Type('x'), window, cx);
    e.execute(EditorAction::Move(Direction::Left), window, cx);
    e.execute(EditorAction::Backspace, window, cx);
    e.execute(EditorAction::Enter, window, cx);
});
```

### State Queries

```rust
editor.read(cx).cursor_position();    // Current cursor byte offset
editor.read(cx).selection_range();    // None if collapsed, Some(Range) if selecting
editor.read(cx).is_dirty();           // Modified since last mark_clean()
editor.read(cx).can_undo();
editor.read(cx).can_redo();
```

## Architecture

The buffer stores raw markdown text using ropey, a rope data structure that provides O(log n) insertions and deletions. On every edit, tree-sitter incrementally reparses the document. Tree-sitter-md produces two parse trees: a block tree representing document structure (paragraphs, headings, lists, code blocks) and separate inline trees for each paragraph's inline content (bold, italic, links). The parser maintains both trees and provides a unified cursor that transparently switches between them when traversing.

Line information is derived from the parse tree. A preorder traversal collects all nodes in document order, then for each line, binary search finds the relevant nodes and extracts markers. Each line has a list of markers representing block-level syntax elements—a task item inside a blockquote has two markers: `[Checkbox, BlockQuote]` (innermost to outermost). Each marker knows its byte range (the bytes to hide when the cursor is away), its visual substitution (e.g., `-` becomes `•`), and its continuation text for smart enter.

The line component renders each line independently. It determines whether to show or hide markers based on cursor position: if the cursor is on the line, raw markdown syntax is visible for editing; otherwise, markers are hidden and substitutions are shown. For inline styles like bold or italic, the same logic applies per-span. Click handling maps visual positions back to buffer offsets by accounting for hidden characters.

### Incremental Parsing

Tree-sitter's incremental parsing is central to writ's responsiveness. When you type a character, tree-sitter doesn't reparse the entire document. Instead, the buffer tells tree-sitter what changed (the byte range and new content), and tree-sitter reuses unchanged portions of the previous syntax tree. The complexity is O(log n + k) where n is the document size and k is the size of the change, rather than O(n) for a full reparse. This means editing a 10,000-line document feels the same as editing a 100-line document.

### Code Block Syntax Highlighting

Code blocks are highlighted using tree-sitter-highlight with language-specific grammars. The editor walks the markdown AST to find fenced code blocks, extracts their content along with the language identifier from the fence line, and highlights each block separately using the appropriate grammar.

This manual extraction approach was chosen over tree-sitter's built-in injection support, which proved unreliable for our use case. Editors like Zed and Helix build their own injection handling for similar reasons. The manual approach is simpler: we find code blocks, highlight them independently, and merge the results back with buffer-relative offsets.

Currently only Rust is supported, but adding new languages requires just the grammar crate and a highlights.scm query file. Highlights are cached and only recomputed after edits.

## Known Issues

### Short Headings Not Styled While Typing

When typing `# Hello`, tree-sitter doesn't recognize it as a heading until enough content is present or a newline is added. This is a quirk of the tree-sitter-md grammar. The heading styling appears once you press Enter or type enough characters.

### Ordered List Continuation Shows Wrong Number

Pressing Shift+Enter on an ordered list item inserts `1. ` as a placeholder. The correct number appears after you start typing, when tree-sitter recognizes the list structure and auto-numbering corrects it.

| 姓名 | 年龄 | 城市 |
| :--- | :--: | ---: |
| 张三 | 25| 北京 |
| 李四 | 30| 上海 |
| 王五 | 28| 广州 |


