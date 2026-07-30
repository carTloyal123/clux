//! Resizing, link resolution, and reading rows for the wire.

use std::collections::HashMap;

use crate::buffer::ViewRow;
use crate::urls::LinkRun;
impl super::Terminal {
    /// Resize the terminal.
    ///
    /// The buffer re-wraps its content, history included, and reports where the
    /// cursor's character ended up.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let (cursor_row, cursor_col) =
            self.buffer
                .resize(rows, cols, (self.cursor.row, self.cursor.col));
        self.cursor.row = cursor_row.min(rows.saturating_sub(1));
        self.cursor.col = cursor_col.min(cols.saturating_sub(1));

        if let Some(alt) = self.alt_primary.as_mut() {
            alt.resize(rows, cols, (0, 0));
        }

        // Update scroll region
        self.scroll_bottom = rows;
        if self.scroll_top >= rows {
            self.scroll_top = 0;
        }

        // Update tab stops
        self.tabs.resize(cols, false);
        for i in (0..cols).step_by(8) {
            self.tabs[i] = true;
        }
    }
    /// Resolve the hyperlinks covering `rows`, following soft-wrap continuations.
    ///
    /// Resolves against whatever the pane is showing, so links keep working while
    /// scrolled back through history. `salt` scopes the generated OSC 8 ids to this
    /// pane. The result can cover rows outside `rows` when a link wraps onto them;
    /// those rows need repainting too. See [`crate::urls`] for why the
    /// multiplexer, not the outer terminal, has to do this.
    pub fn resolve_links(
        &self,
        salt: u32,
        detect_plain_urls: bool,
        rows: &[u16],
    ) -> HashMap<u16, Vec<LinkRun>> {
        crate::urls::resolve_links(
            &self.buffer,
            &self.hyperlinks,
            salt,
            detect_plain_urls,
            rows,
        )
    }
    /// The row the user sees at this screen position.
    ///
    /// Comes from the scrollback when the view is scrolled back, otherwise from
    /// the live grid - see [`crate::scrollview`]. Everything that serializes a
    /// pane row goes through here so the two cannot drift apart.
    pub fn view_row(&self, row_idx: u16) -> ViewRow {
        self.buffer.view_row(row_idx)
    }
}
