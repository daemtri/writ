use std::collections::HashMap;
use std::num::NonZeroU16;

use tree_sitter::{InputEdit, Language, Node, Parser, Point, Range, Tree, TreeCursor};
use tree_sitter_md::{INLINE_LANGUAGE, LANGUAGE};

pub struct MarkdownParser {
    parser: Parser,
    block_language: Language,
    inline_language: Language,
}

pub struct MarkdownCursor<'a> {
    markdown_tree: &'a MarkdownTree,
    block_cursor: TreeCursor<'a>,
    inline_cursor: Option<TreeCursor<'a>>,
}

impl<'a> MarkdownCursor<'a> {
    pub fn node(&self) -> Node<'a> {
        match &self.inline_cursor {
            Some(cursor) => cursor.node(),
            None => self.block_cursor.node(),
        }
    }

    pub fn is_inline(&self) -> bool {
        self.inline_cursor.is_some()
    }

    pub fn field_id(&self) -> Option<NonZeroU16> {
        match &self.inline_cursor {
            Some(cursor) => cursor.field_id(),
            None => self.block_cursor.field_id(),
        }
    }

    pub fn field_name(&self) -> Option<&'static str> {
        match &self.inline_cursor {
            Some(cursor) => cursor.field_name(),
            None => self.block_cursor.field_name(),
        }
    }

    fn move_to_inline_tree(&mut self) -> bool {
        let node = self.block_cursor.node();
        match node.kind() {
            "inline" | "pipe_table_cell" => {
                if let Some(inline_tree) = self.markdown_tree.inline_tree(&node) {
                    self.inline_cursor = Some(inline_tree.walk());
                    return true;
                }
            }
            _ => (),
        }
        false
    }

    fn move_to_block_tree(&mut self) {
        self.inline_cursor = None;
    }

    pub fn goto_first_child(&mut self) -> bool {
        match &mut self.inline_cursor {
            Some(cursor) => cursor.goto_first_child(),
            None => {
                if self.move_to_inline_tree() {
                    if !self.inline_cursor.as_mut().unwrap().goto_first_child() {
                        self.move_to_block_tree();
                        false
                    } else {
                        true
                    }
                } else {
                    self.block_cursor.goto_first_child()
                }
            }
        }
    }

    pub fn goto_parent(&mut self) -> bool {
        match &mut self.inline_cursor {
            Some(inline_cursor) => {
                inline_cursor.goto_parent();
                if inline_cursor.node().parent().is_none() {
                    self.move_to_block_tree();
                }
                true
            }
            None => self.block_cursor.goto_parent(),
        }
    }

    pub fn goto_next_sibling(&mut self) -> bool {
        match &mut self.inline_cursor {
            Some(inline_cursor) => inline_cursor.goto_next_sibling(),
            None => self.block_cursor.goto_next_sibling(),
        }
    }

    pub fn goto_first_child_for_byte(&mut self, index: usize) -> Option<usize> {
        match &mut self.inline_cursor {
            Some(cursor) => cursor.goto_first_child_for_byte(index),
            None => {
                if self.move_to_inline_tree() {
                    self.inline_cursor
                        .as_mut()
                        .unwrap()
                        .goto_first_child_for_byte(index)
                } else {
                    self.block_cursor.goto_first_child_for_byte(index)
                }
            }
        }
    }

    pub fn goto_first_child_for_point(&mut self, index: Point) -> Option<usize> {
        match &mut self.inline_cursor {
            Some(cursor) => cursor.goto_first_child_for_point(index),
            None => {
                if self.move_to_inline_tree() {
                    self.inline_cursor
                        .as_mut()
                        .unwrap()
                        .goto_first_child_for_point(index)
                } else {
                    self.block_cursor.goto_first_child_for_point(index)
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct MarkdownTree {
    block_tree: Tree,
    inline_trees: Vec<Tree>,
    inline_indices: HashMap<usize, usize>,
}

impl MarkdownTree {
    pub fn edit(&mut self, edit: &InputEdit) {
        self.block_tree.edit(edit);
        for inline_tree in self.inline_trees.iter_mut() {
            inline_tree.edit(edit);
        }
    }

    pub fn block_tree(&self) -> &Tree {
        &self.block_tree
    }

    pub fn inline_tree(&self, parent: &Node) -> Option<&Tree> {
        let index = *self.inline_indices.get(&parent.id())?;
        Some(&self.inline_trees[index])
    }

    pub fn inline_trees(&self) -> &[Tree] {
        &self.inline_trees
    }

    pub fn walk(&self) -> MarkdownCursor<'_> {
        MarkdownCursor {
            markdown_tree: self,
            block_cursor: self.block_tree.walk(),
            inline_cursor: None,
        }
    }
}

impl Default for MarkdownParser {
    fn default() -> Self {
        let block_language = LANGUAGE.into();
        let inline_language = INLINE_LANGUAGE.into();
        let parser = Parser::new();
        MarkdownParser {
            parser,
            block_language,
            inline_language,
        }
    }
}

impl MarkdownParser {
    pub fn parse_with<T: AsRef<[u8]>, F: FnMut(usize, Point) -> T>(
        &mut self,
        callback: &mut F,
        old_tree: Option<&MarkdownTree>,
    ) -> Option<MarkdownTree> {
        let MarkdownParser {
            parser,
            block_language,
            inline_language,
        } = self;
        parser
            .set_included_ranges(&[])
            .expect("Can not set included ranges to whole document");
        parser
            .set_language(block_language)
            .expect("Could not load block grammar");
        let block_tree = parser.parse_with_options(
            callback,
            old_tree.map(|tree| &tree.block_tree),
            None, // No progress callback
        )?;
        let (mut inline_trees, mut inline_indices) = if let Some(old_tree) = old_tree {
            let len = old_tree.inline_trees.len();
            (Vec::with_capacity(len), HashMap::with_capacity(len))
        } else {
            (Vec::new(), HashMap::new())
        };
        parser
            .set_language(inline_language)
            .expect("Could not load inline grammar");
        let mut tree_cursor = block_tree.walk();

        let mut i = 0;
        'outer: loop {
            let node = loop {
                let kind = tree_cursor.node().kind();
                if kind == "inline" || kind == "pipe_table_cell" || !tree_cursor.goto_first_child()
                {
                    while !tree_cursor.goto_next_sibling() {
                        if !tree_cursor.goto_parent() {
                            break 'outer;
                        }
                    }
                }
                let kind = tree_cursor.node().kind();
                if kind == "inline" || kind == "pipe_table_cell" {
                    break tree_cursor.node();
                }
            };
            let mut range = node.range();
            let mut ranges = Vec::new();
            if tree_cursor.goto_first_child() {
                while tree_cursor.goto_next_sibling() {
                    if !tree_cursor.node().is_named() {
                        continue;
                    }
                    let child_range = tree_cursor.node().range();
                    ranges.push(Range {
                        start_byte: range.start_byte,
                        start_point: range.start_point,
                        end_byte: child_range.start_byte,
                        end_point: child_range.start_point,
                    });
                    range.start_byte = child_range.end_byte;
                    range.start_point = child_range.end_point;
                }
                tree_cursor.goto_parent();
            }
            ranges.push(range);
            parser.set_included_ranges(&ranges).ok()?;
            let inline_tree = parser.parse_with_options(
                callback,
                old_tree.and_then(|old_tree| old_tree.inline_trees.get(i)),
                None, // No progress callback
            )?;
            inline_trees.push(inline_tree);
            inline_indices.insert(node.id(), i);
            i += 1;
        }
        drop(tree_cursor);
        inline_trees.shrink_to_fit();
        inline_indices.shrink_to_fit();
        Some(MarkdownTree {
            block_tree,
            inline_trees,
            inline_indices,
        })
    }

    #[cfg(test)]
    pub fn parse(&mut self, text: &[u8], old_tree: Option<&MarkdownTree>) -> Option<MarkdownTree> {
        self.parse_with(&mut |byte, _| &text[byte..], old_tree)
    }

    pub fn parse_rope(
        &mut self,
        rope: &ropey::Rope,
        old_tree: Option<&MarkdownTree>,
    ) -> Option<MarkdownTree> {
        self.parse_with(
            &mut |byte, _| {
                // tree-sitter may request data at or past EOF. Ropey panics if
                // chunk_at_byte is called with an out-of-bounds index, and that
                // panic cannot unwind through the FFI boundary.
                let len = rope.len_bytes();
                if byte >= len {
                    let empty: &[u8] = &[];
                    return empty;
                }

                let (chunk, chunk_start, _, _) = rope.chunk_at_byte(byte);
                &chunk.as_bytes()[byte - chunk_start..]
            },
            old_tree,
        )
    }
}

#[cfg(test)]
mod tests {
    use ropey::Rope;
    use tree_sitter::{InputEdit, Point};

    use super::*;

    #[test]
    fn inline_ranges() {
        let code = "# title\n\nInline [content].\n";
        let mut parser = MarkdownParser::default();
        let mut tree = parser.parse(code.as_bytes(), None).unwrap();

        let section = tree.block_tree().root_node().child(0).unwrap();
        assert_eq!(section.kind(), "section");
        let heading = section.child(0).unwrap();
        assert_eq!(heading.kind(), "atx_heading");
        let paragraph = section.child(1).unwrap();
        assert_eq!(paragraph.kind(), "paragraph");
        let inline = paragraph.child(0).unwrap();
        assert_eq!(inline.kind(), "inline");
        assert_eq!(
            tree.inline_tree(&inline)
                .unwrap()
                .root_node()
                .child(0)
                .unwrap()
                .kind(),
            "shortcut_link"
        );

        let code = "# Title\n\nInline [content].\n";
        tree.edit(&InputEdit {
            start_byte: 2,
            old_end_byte: 3,
            new_end_byte: 3,
            start_position: Point { row: 0, column: 2 },
            old_end_position: Point { row: 0, column: 3 },
            new_end_position: Point { row: 0, column: 3 },
        });
        let tree = parser.parse(code.as_bytes(), Some(&tree)).unwrap();

        let section = tree.block_tree().root_node().child(0).unwrap();
        assert_eq!(section.kind(), "section");
        let heading = section.child(0).unwrap();
        assert_eq!(heading.kind(), "atx_heading");
        let paragraph = section.child(1).unwrap();
        assert_eq!(paragraph.kind(), "paragraph");
        let inline = paragraph.child(0).unwrap();
        assert_eq!(inline.kind(), "inline");
        assert_eq!(
            tree.inline_tree(&inline)
                .unwrap()
                .root_node()
                .named_child(0)
                .unwrap()
                .kind(),
            "shortcut_link"
        );
    }

    #[test]
    fn markdown_cursor() {
        let code = "# title\n\nInline [content].\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(code.as_bytes(), None).unwrap();
        let mut cursor = tree.walk();
        assert_eq!(cursor.node().kind(), "document");
        assert!(cursor.goto_first_child());
        assert_eq!(cursor.node().kind(), "section");
        assert!(cursor.goto_first_child());
        assert_eq!(cursor.node().kind(), "atx_heading");
        assert!(cursor.goto_next_sibling());
        assert_eq!(cursor.node().kind(), "paragraph");
        assert!(cursor.goto_first_child());
        assert_eq!(cursor.node().kind(), "inline");
        assert!(cursor.goto_first_child());
        assert_eq!(cursor.node().kind(), "shortcut_link");
        assert!(cursor.goto_parent());
        assert!(cursor.goto_parent());
        assert!(cursor.goto_parent());
        assert!(cursor.goto_parent());
        assert_eq!(cursor.node().kind(), "document");
    }

    #[test]
    fn parse_rope_does_not_panic_on_out_of_bounds_requests() {
        // If an incremental edit is misapplied, tree-sitter may request input at
        // an out-of-bounds byte offset. Our rope-backed callback must never panic
        // (panics cannot unwind across the FFI boundary).
        let rope = Rope::from_str("a");
        let mut parser = MarkdownParser::default();

        let mut tree = parser.parse_rope(&rope, None).unwrap();
        tree.edit(&InputEdit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 3,
            start_position: Point { row: 0, column: 0 },
            old_end_position: Point { row: 0, column: 0 },
            new_end_position: Point { row: 0, column: 3 },
        });

        assert!(parser.parse_rope(&rope, Some(&tree)).is_some());
    }

    #[test]
    fn table() {
        let code = "| foo |\n| --- |\n| *bar*|\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(code.as_bytes(), None).unwrap();
        dbg!(&tree.inline_trees());
        let mut cursor = tree.walk();

        assert_eq!(cursor.node().kind(), "document");
        assert!(cursor.goto_first_child());
        assert_eq!(cursor.node().kind(), "section");
        assert!(cursor.goto_first_child());
        assert_eq!(cursor.node().kind(), "pipe_table");
        assert!(cursor.goto_first_child());
        assert!(cursor.goto_next_sibling());
        assert!(cursor.goto_next_sibling());
        assert_eq!(cursor.node().kind(), "pipe_table_row");
        assert!(cursor.goto_first_child());
        assert!(cursor.goto_next_sibling());
        assert_eq!(cursor.node().kind(), "pipe_table_cell");
        assert!(cursor.goto_first_child());
        assert_eq!(cursor.node().kind(), "emphasis");
    }

    #[test]
    fn changed_ranges_blockquote() {
        // Test: what does changed_ranges report when we remove a blockquote marker?
        let code1 = "> line one\n> line two\n> line three\n";
        let mut parser = MarkdownParser::default();
        let tree1 = parser.parse(code1.as_bytes(), None).unwrap();

        // Remove the `> ` from line one, making it a plain paragraph
        let code2 = "line one\n> line two\n> line three\n";
        let mut tree1_edited = parser.parse(code1.as_bytes(), None).unwrap();
        tree1_edited.edit(&InputEdit {
            start_byte: 0,
            old_end_byte: 2,
            new_end_byte: 0,
            start_position: Point { row: 0, column: 0 },
            old_end_position: Point { row: 0, column: 2 },
            new_end_position: Point { row: 0, column: 0 },
        });
        let tree2 = parser.parse(code2.as_bytes(), Some(&tree1_edited)).unwrap();

        let ranges: Vec<_> = tree2
            .block_tree()
            .changed_ranges(tree1.block_tree())
            .collect();
        eprintln!("Changed ranges after removing first blockquote marker:");
        for range in &ranges {
            eprintln!(
                "  bytes {}..{} (rows {}..{})",
                range.start_byte, range.end_byte, range.start_point.row, range.end_point.row
            );
        }

        // We expect it to report at least line 0 changed.
        // The question is: does it also report lines 1-2 changed since they're
        // now a separate blockquote?
        assert!(!ranges.is_empty());
    }
}
