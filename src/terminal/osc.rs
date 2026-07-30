//! OSC sequence handling, including OSC 8 hyperlinks.

use super::Terminal;

impl Terminal {
    pub(super) fn dispatch_osc(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        // Parse first parameter as command number
        let cmd = std::str::from_utf8(params[0])
            .ok()
            .and_then(|s| s.parse::<u32>().ok());

        match cmd {
            // Set window title (OSC 0, 1, 2)
            Some(0) | Some(1) | Some(2) => {
                // Title setting - we could emit an event here
            }
            // OSC 8 - hyperlinks
            Some(8) => {
                // OSC 8 format: ESC ] 8 ; params ; URI ST
                // params[0] = "8", params[1] = id/params, params[2..] = URI
                //
                // The parser splits on ';', but ';' is legal inside a URI
                // (matrix parameters, mailto headers), so rejoin the tail
                // instead of truncating the URL at the first one.
                if params.len() >= 2 {
                    let uri = if params.len() >= 3 {
                        join_semicolons(&params[2..])
                    } else {
                        // Empty URI closes hyperlink
                        std::str::from_utf8(params[1]).ok().map(str::to_string)
                    };

                    match uri.as_deref().and_then(crate::urls::sanitize_url) {
                        Some(url) => {
                            // Open hyperlink - intern the URL
                            let id = self.hyperlinks.intern(&url);
                            self.hyperlink = Some(id);
                        }
                        None => {
                            // Close hyperlink
                            self.hyperlink = None;
                        }
                    }
                } else {
                    // Malformed - close hyperlink
                    self.hyperlink = None;
                }
            }
            _ => {}
        }
    }
}

/// Rejoin OSC parameters that the parser split on a ';' belonging to the payload.
fn join_semicolons(params: &[&[u8]]) -> Option<String> {
    let parts: Option<Vec<&str>> = params.iter().map(|p| std::str::from_utf8(p).ok()).collect();

    Some(parts?.join(";"))
}
