//! Selection tests.

use super::super::*;
use super::test_support::*;
use crate::cell::CellFlags;
use crate::selection::SelectionMode;

#[test]
fn test_selection_extracts_text_on_one_row() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(single_pane_layout(20, 2));
    buffer.apply_pane_update(0, &[pane_row(0, "hello world", false)]);

    assert!(buffer.begin_selection(0, 0, SelectionMode::Normal));
    buffer.extend_selection(0, 4);

    assert_eq!(buffer.selected_text().as_deref(), Some("hello"));
    assert!(buffer.has_selection());
}

#[test]
fn test_selection_joins_wrapped_rows_without_a_newline() {
    // The point of shipping the wrap flag: a path broken across rows must come
    // back as one string.
    let mut buffer = ScreenBuffer::new(10, 3);
    buffer.set_layout(single_pane_layout(10, 3));
    buffer.apply_pane_update(
        0,
        &[
            pane_row(0, "/very/long", true),
            pane_row(1, "/path/here", false),
        ],
    );

    buffer.begin_selection(0, 0, SelectionMode::Normal);
    buffer.extend_selection(1, 9);

    assert_eq!(
        buffer.selected_text().as_deref(),
        Some("/very/long/path/here")
    );
}

#[test]
fn test_selection_breaks_unwrapped_rows_with_a_newline() {
    let mut buffer = ScreenBuffer::new(10, 3);
    buffer.set_layout(single_pane_layout(10, 3));
    buffer.apply_pane_update(
        0,
        &[pane_row(0, "first", false), pane_row(1, "second", false)],
    );

    buffer.begin_selection(0, 0, SelectionMode::Normal);
    buffer.extend_selection(1, 9);

    assert_eq!(buffer.selected_text().as_deref(), Some("first\nsecond"));
}

#[test]
fn test_selection_trims_trailing_blanks_at_hard_line_ends() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(single_pane_layout(20, 2));
    buffer.apply_pane_update(0, &[pane_row(0, "text", false)]);

    buffer.begin_selection(0, 0, SelectionMode::Normal);
    buffer.extend_selection(0, 19);

    assert_eq!(buffer.selected_text().as_deref(), Some("text"));
}

#[test]
fn test_selection_stays_inside_its_pane() {
    let mut buffer = ScreenBuffer::new(21, 3);
    buffer.set_layout(split_layout());
    buffer.apply_pane_update(0, &[pane_row(0, "LEFTPANE00", false)]);
    buffer.apply_pane_update(1, &[pane_row(0, "RIGHTPANE0", false)]);

    // Start in the left pane, drag well into the right one.
    buffer.begin_selection(0, 0, SelectionMode::Normal);
    buffer.extend_selection(0, 20);

    let text = buffer.selected_text().expect("selection");
    assert_eq!(text, "LEFTPANE00");
    assert!(!text.contains('│'), "selection swallowed the divider");
    assert!(!text.contains("RIGHT"), "selection crossed into pane 1");
}

#[test]
fn test_selection_on_a_divider_does_not_start() {
    let mut buffer = ScreenBuffer::new(21, 3);
    buffer.set_layout(split_layout());

    assert!(!buffer.begin_selection(0, 10, SelectionMode::Normal));
    assert!(!buffer.has_selection());
    assert_eq!(buffer.selected_text(), None);
}

#[test]
fn test_block_selection_keeps_rows_separate() {
    let mut buffer = ScreenBuffer::new(10, 3);
    buffer.set_layout(single_pane_layout(10, 3));
    // Wrapped rows, but a block selection is columnar: rows stay separate.
    buffer.apply_pane_update(
        0,
        &[
            pane_row(0, "abcdefghij", true),
            pane_row(1, "klmnopqrst", true),
        ],
    );

    buffer.begin_selection(0, 2, SelectionMode::Block);
    buffer.extend_selection(1, 4);

    assert_eq!(buffer.selected_text().as_deref(), Some("cde\nmno"));
}

#[test]
fn test_selected_cells_are_inverted_in_the_rendered_row() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(single_pane_layout(20, 2));
    buffer.apply_pane_update(0, &[pane_row(0, "hello world", false)]);

    buffer.begin_selection(0, 0, SelectionMode::Normal);
    buffer.extend_selection(0, 4);

    let ansi = buffer.render_row_ansi(0);
    assert!(ansi.contains("\x1b[0;7m"), "no inverse SGR in {ansi:?}");

    // The stored cells are untouched, so clearing needs no restore.
    assert!(!buffer.get_row(0).unwrap()[0]
        .flags
        .contains(CellFlags::INVERSE));

    buffer.clear_selection();
    assert!(!buffer.render_row_ansi(0).contains("\x1b[0;7m"));
}

#[test]
fn test_selection_survives_a_pane_update_but_not_a_layout_change() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(single_pane_layout(20, 2));
    buffer.apply_pane_update(0, &[pane_row(0, "hello world", false)]);

    buffer.begin_selection(0, 0, SelectionMode::Normal);
    buffer.extend_selection(0, 4);

    // New output on another row must not drop what the user selected.
    buffer.apply_pane_update(0, &[pane_row(1, "more output", false)]);
    assert_eq!(buffer.selected_text().as_deref(), Some("hello"));

    // A layout change moves everything, so the selection is meaningless.
    buffer.set_layout(single_pane_layout(20, 2));
    assert!(!buffer.has_selection());
}
