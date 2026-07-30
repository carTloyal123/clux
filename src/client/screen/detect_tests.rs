//! Hyperlink emission tests.

use super::super::*;
use super::test_support::*;
use crate::cell::CellFlags;
use crate::protocol::{PaneLayout, PaneRow, WindowLayout};

#[test]
fn test_detected_url_is_underlined() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(single_pane_layout(20, 2));

    buffer.apply_pane_update(
        0,
        &[PaneRow::with_links(
            0,
            text_cells("go https://a.io x"),
            vec![link(3, 15, 1, "https://a.io")],
        )],
    );

    let row = buffer.get_row(0).unwrap();
    assert!(
        row[3].flags.contains(CellFlags::UNDERLINE),
        "detected URL should be underlined"
    );
    assert!(
        !row[0].flags.contains(CellFlags::UNDERLINE),
        "underline leaked outside the link"
    );
    assert!(
        !row[16].flags.contains(CellFlags::UNDERLINE),
        "underline leaked past the link"
    );
    // ...and the underline reaches the host terminal as an SGR.
    let ansi = buffer.render_row_ansi(0);
    assert!(ansi.contains("\x1b[0;4m"), "no underline SGR in {ansi:?}");
}

#[test]
fn test_application_link_keeps_its_own_styling() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(single_pane_layout(20, 2));

    // The application printed plain, unstyled text and asked for a link on it.
    buffer.apply_pane_update(
        0,
        &[PaneRow::with_links(
            0,
            text_cells("CLICKME"),
            vec![app_link(0, 7, 2, "https://a.io/osc8")],
        )],
    );

    let row = buffer.get_row(0).unwrap();
    assert!(
        !row[0].flags.contains(CellFlags::UNDERLINE),
        "clux must not restyle an application's own link"
    );
    // It is still a real hyperlink, just not restyled.
    assert_eq!(buffer.link_at(0, 0), Some("https://a.io/osc8"));
}

#[test]
fn test_underline_is_dropped_when_the_link_goes_away() {
    let mut buffer = ScreenBuffer::new(20, 2);
    buffer.set_layout(single_pane_layout(20, 2));

    buffer.apply_pane_update(
        0,
        &[PaneRow::with_links(
            0,
            text_cells("https://a.io"),
            vec![link(0, 12, 1, "https://a.io")],
        )],
    );
    buffer.apply_pane_update(0, &[PaneRow::new(0, text_cells("plain text"))]);

    let row = buffer.get_row(0).unwrap();
    assert!(!row[0].flags.contains(CellFlags::UNDERLINE));
}

// ------------------------------------------------------------------------
// Selection
// ------------------------------------------------------------------------
