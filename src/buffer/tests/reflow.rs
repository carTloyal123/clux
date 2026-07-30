//! Resize and re-wrapping, history included.

use super::*;

/// Write one long logical line by filling rows and flagging them as continuing.
fn write_wrapped(buffer: &mut Buffer, row: usize, text: &str) {
    let cols = buffer.cols();
    let mut row = row;
    for chunk in text.as_bytes().chunks(cols) {
        write_row(buffer, row, std::str::from_utf8(chunk).unwrap());
        if chunk.len() == cols {
            buffer.set_row_wrapped(row, true);
        }
        row += 1;
    }
}

#[test]
fn widening_puts_a_wrapped_line_back_together() {
    let mut buffer = Buffer::new(4, 10, 100 * 64);
    write_wrapped(&mut buffer, 0, "0123456789abcdefghij");
    assert_eq!(viewport_text(&buffer, 0), "0123456789");

    buffer.resize(4, 20, (0, 0));

    assert_eq!(
        viewport_text(&buffer, 0),
        "0123456789abcdefghij",
        "widening should rejoin the line"
    );
    assert!(!buffer.row_wrapped(0), "it no longer needs to wrap");
}

#[test]
fn narrowing_wraps_a_line_and_flags_it() {
    let mut buffer = Buffer::new(4, 20, 100 * 64);
    write_row(&mut buffer, 0, "0123456789abcdefghij");

    buffer.resize(4, 10, (0, 0));

    assert_eq!(viewport_text(&buffer, 0), "0123456789");
    assert_eq!(viewport_text(&buffer, 1), "abcdefghij");
    assert!(buffer.row_wrapped(0), "the first half continues");
    assert!(!buffer.row_wrapped(1));
}

#[test]
fn a_width_round_trip_preserves_content() {
    let mut buffer = Buffer::new(5, 20, 100 * 64);
    write_row(&mut buffer, 0, "the quick brown fox");
    write_row(&mut buffer, 1, "jumps over the lazy");
    let before = viewport(&buffer);

    buffer.resize(5, 7, (0, 0));
    buffer.resize(5, 20, (0, 0));

    assert_eq!(viewport(&buffer), before, "content survived the round trip");
}

#[test]
fn history_is_rewrapped_too() {
    // The whole point: old output re-wraps when the window changes width.
    let mut buffer = Buffer::new(4, 10, 100 * 64);
    write_wrapped(&mut buffer, 0, "0123456789abcdefghij");
    // Push it into history, without writing over its second row.
    for _ in 0..6 {
        buffer.scroll_up();
    }

    buffer.resize(2, 20, (0, 0));

    // Scroll back and find the line whole.
    let mut found = false;
    for offset in 0..=buffer.history_rows() as i32 {
        buffer.reset_scroll();
        buffer.scroll_view(offset);
        for row in 0..buffer.screen_rows() {
            if viewport_text(&buffer, row) == "0123456789abcdefghij" {
                found = true;
            }
        }
    }
    assert!(
        found,
        "history should have been re-wrapped at the new width"
    );
}

#[test]
fn the_cursor_follows_its_character_through_a_reflow() {
    let mut buffer = Buffer::new(4, 10, 100 * 64);
    write_wrapped(&mut buffer, 0, "0123456789abcdefghij");

    // Cursor on 'c', the third character of the second row.
    let (row, col) = buffer.resize(4, 20, (1, 2));

    assert_eq!(
        buffer.cell(row, col).map(|c| c.c),
        Some('c'),
        "the cursor should still be on the same character"
    );
}

#[test]
fn growing_the_screen_pulls_rows_back_from_history() {
    let mut buffer = Buffer::new(2, 10, 100 * 64);
    print_lines(&mut buffer, 6);
    let history_before = buffer.history_rows();

    // Cursor on the bottom row, as a shell leaves it.
    let (row, _) = buffer.resize(4, 10, (1, 0));

    assert_eq!(buffer.screen_rows(), 4);
    assert_eq!(
        buffer.history_rows(),
        history_before - 2,
        "growing should reveal history, not append blank rows"
    );
    assert_eq!(
        row, 3,
        "the cursor keeps its content and moves down the screen"
    );
}

#[test]
fn shrinking_reclaims_blank_rows_before_touching_content() {
    // A screen with content at the top and blanks below - the common case after a
    // few commands. Shrinking must not push that content into history.
    let mut buffer = Buffer::new(10, 10, 100 * 64);
    for row in 0..4 {
        write_row(&mut buffer, row, &format!("line {}", row));
    }

    let (row, _) = buffer.resize(6, 10, (3, 0));

    assert_eq!(
        buffer.history_rows(),
        0,
        "blank rows should absorb the change"
    );
    assert_eq!(row, 3, "the cursor stays on its line");
    assert_eq!(viewport_text(&buffer, 0), "line 0");
}

#[test]
fn shrinking_the_screen_pushes_rows_into_history() {
    let mut buffer = Buffer::new(4, 10, 100 * 64);
    print_lines(&mut buffer, 6);
    let history_before = buffer.history_rows();

    // Cursor on the bottom row: there are no blanks below it to reclaim, so the
    // top of the screen becomes history.
    buffer.resize(2, 10, (3, 0));

    assert_eq!(buffer.screen_rows(), 2);
    assert_eq!(buffer.history_rows(), history_before + 2);
}

#[test]
fn a_no_history_buffer_resizes_without_reflow() {
    // The alternate screen: applications redraw, so content is not re-wrapped.
    let mut buffer = Buffer::new(3, 10, 0);
    write_row(&mut buffer, 0, "0123456789");

    buffer.resize(3, 5, (0, 0));

    assert_eq!(buffer.cols(), 5);
    assert_eq!(
        viewport_text(&buffer, 0),
        "01234",
        "clipped, not re-wrapped"
    );
    assert_eq!(buffer.history_rows(), 0);
}
