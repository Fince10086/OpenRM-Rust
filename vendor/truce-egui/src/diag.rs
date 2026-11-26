//! Minimal diagnostic sink for editor lifecycle investigation. Appends to
//! `/tmp/openrm-editor.log`; off the audio thread only.

use std::io::Write;

pub(crate) fn trace(line: impl AsRef<str>) {
    let path = std::path::Path::new("/tmp/openrm-editor.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", line.as_ref());
    }
}
