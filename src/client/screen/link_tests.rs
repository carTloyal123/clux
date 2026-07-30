//! Hyperlink emission tests.

use super::super::*;
use super::test_support::*;
use crate::protocol::{PaneLayout, PaneRow, WindowLayout};

#[test]
fn test_row_emits_osc8_hyperlink() {
    let mut buffer = ScreenBuffer::new(20, 3);
    buffer.set_layout(single_pane_layout(20, 3));

    buffer.apply_pane_update(
        0,
        &[PaneRow::with_links(
            0,
            text_cells("go https://a.io x"),
            vec![link(3, 15, 42, "https://a.io")],
        )],
    );

    let ansi = buffer.render_row_ansi(0);
    assert!(
        ansi.contains("\x1b]8;id=42;https://a.io\x1b\\"),
        "no OSC 8 open in {ansi:?}"
    );
    assert!(
        ansi.contains("\x1b]8;;\x1b\\"),
        "no OSC 8 close in {ansi:?}"
    );

    // The link must cover exactly the URL text.
    let opened = ansi
        .split("\x1b]8;id=42;https://a.io\x1b\\")
        .nth(1)
        .unwrap();
    let linked: String = opened
        .split("\x1b]8;;\x1b\\")
        .next()
        .unwrap()
        .chars()
        .filter(|c| !c.is_control() && *c != '[' || c.is_alphanumeric())
        .collect();
    assert!(
        linked.contains("https://a.io"),
        "linked text was {linked:?}"
    );
    assert!(!linked.contains(" x"), "link ran past the URL: {linked:?}");
}

#[test]
fn test_link_closes_at_end_of_row() {
    // A link running to the last column must be closed, or it bleeds into the
    // next row the client paints.
    let mut buffer = ScreenBuffer::new(12, 2);
    buffer.set_layout(single_pane_layout(12, 2));

    buffer.apply_pane_update(
        0,
        &[PaneRow::with_links(
            0,
            text_cells("https://a.io"),
            vec![link(0, 12, 7, "https://a.io")],
        )],
    );

    assert!(
        buffer.render_row_ansi(0).ends_with("\x1b]8;;\x1b[0m")
            || buffer.render_row_ansi(0).contains("\x1b]8;;\x1b\\\x1b[0m")
    );
}

#[test]
fn test_wrapped_link_shares_one_id_across_rows() {
    let mut buffer = ScreenBuffer::new(10, 3);
    buffer.set_layout(single_pane_layout(10, 3));

    // One logical link split over two rows, as the server sends it.
    buffer.apply_pane_update(
        0,
        &[
            PaneRow::with_links(
                0,
                text_cells("https://a."),
                vec![link(0, 10, 99, "https://a.io/x")],
            ),
            PaneRow::with_links(
                1,
                text_cells("io/x"),
                vec![link(0, 4, 99, "https://a.io/x")],
            ),
        ],
    );

    for row in 0..2 {
        assert!(
            buffer.render_row_ansi(row).contains("\x1b]8;id=99;"),
            "row {row} lost the shared link id"
        );
    }
    assert_eq!(buffer.link_at(1, 0), Some("https://a.io/x"));
}

#[test]
fn test_links_are_replaced_when_a_row_is_repainted() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(single_pane_layout(20, 2));

    buffer.apply_pane_update(
        0,
        &[PaneRow::with_links(
            0,
            text_cells("https://old.example"),
            vec![link(0, 19, 1, "https://old.example")],
        )],
    );
    buffer.apply_pane_update(0, &[PaneRow::new(0, text_cells("plain text"))]);

    assert_eq!(buffer.link_at(0, 0), None, "stale link survived a repaint");
    assert!(!buffer.render_row_ansi(0).contains("\x1b]8;"));
}

#[test]
fn test_link_is_clipped_to_its_pane() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(WindowLayout {
        panes: vec![
            PaneLayout {
                pane_id: 0,
                x: 0,
                y: 0,
                width: 10,
                height: 2,
                focused: true,
            },
            PaneLayout {
                pane_id: 1,
                x: 11,
                y: 0,
                width: 9,
                height: 2,
                focused: false,
            },
        ],
        screen_cols: 20,
        screen_rows: 2,
    });

    // A run wider than the pane must not reach the divider or the neighbour.
    buffer.apply_pane_update(
        0,
        &[PaneRow::with_links(
            0,
            text_cells("https://a.io"),
            vec![link(0, 30, 5, "https://a.io")],
        )],
    );

    assert_eq!(buffer.link_at(0, 9), Some("https://a.io"));
    assert_eq!(buffer.link_at(0, 10), None, "link reached the divider");
    assert_eq!(buffer.link_at(0, 12), None, "link reached the next pane");
}

#[test]
fn test_url_with_control_characters_is_dropped() {
    let mut buffer = ScreenBuffer::new(12, 2);
    buffer.set_layout(single_pane_layout(12, 2));

    buffer.apply_pane_update(
        0,
        &[PaneRow::with_links(
            0,
            text_cells("click me"),
            vec![link(0, 8, 3, "https://a.io\x1b]0;pwned\x07")],
        )],
    );

    assert_eq!(buffer.link_at(0, 0), None);
    assert!(!buffer.render_row_ansi(0).contains("\x1b]8;"));
}
