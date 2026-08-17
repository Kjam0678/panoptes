//! What a page reports back to the shell after touching the document.

/// The outcome of an edit: a status line on success, a reason on failure.
/// Pages hand this back rather than writing to the status bar themselves, and
/// `None` from a page means nothing was touched this frame.
pub type Change = Result<String, String>;
