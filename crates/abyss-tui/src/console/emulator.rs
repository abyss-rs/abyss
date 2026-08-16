use std::path::PathBuf;

use percent_encoding::percent_decode;

/// Captures the escape sequences vt100 does not handle itself.
///
/// vt100 dispatches OSC 0/1/2 (titles) and 52 (clipboard) to dedicated
/// callbacks and forwards everything else here, so OSC 7 — the working
/// directory a shell reports on each prompt — arrives intact.
#[derive(Default)]
pub(crate) struct OscCallbacks {
    cwd: Option<PathBuf>,
}

impl OscCallbacks {
    /// Consume the most recent directory the shell reported.
    pub(crate) fn take_cwd(&mut self) -> Option<PathBuf> {
        self.cwd.take()
    }
}

impl vt100::Callbacks for OscCallbacks {
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        if let [b"7", url] = params
            && let Some(path) = parse_osc7(url)
        {
            self.cwd = Some(path);
        }
    }
}

/// Extract the path from an OSC 7 payload: `file://<host>/<path>`.
///
/// The host is ignored — a shell on the far side of an ssh session reports a
/// path that means nothing locally, but neither does refusing to parse it, and
/// callers check that the directory exists before acting on it.
pub(crate) fn parse_osc7(url: &[u8]) -> Option<PathBuf> {
    let rest = url.strip_prefix(b"file://")?;
    // Everything from the slash that ends the host onwards is the path.
    let start = rest.iter().position(|byte| *byte == b'/')?;
    let encoded = &rest[start..];
    if encoded.is_empty() {
        return None;
    }
    let decoded = percent_decode(encoded).collect::<Vec<u8>>();
    let text = String::from_utf8(decoded).ok()?;
    if text.is_empty() {
        return None;
    }
    Some(PathBuf::from(text))
}
