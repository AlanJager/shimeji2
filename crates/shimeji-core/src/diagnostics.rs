//! Load and runtime diagnostics.
//!
//! A diagnostic names the pack and the file that produced it. `line` is the
//! 1-based line in that file when the record came from XML. `fallback` is set
//! when the loader substituted a behavior the reference would have rejected,
//! which today is only an unknown embedded class rewritten to Stay.

/// How serious the record is. Info does not mean the pack is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// What the record is about. The corpus summary groups on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Xml,
    Expression,
    UnknownClass,
    FastFollowClass,
    MissingImage,
    ImageEscape,
    UnresolvedEntity,
    MissingConfig,
    MissingReference,
    MissingAttribute,
    UnknownActionType,
    Other,
}

/// One load or runtime problem, with enough context to point at the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: DiagnosticKind,
    /// Pack directory name.
    pub pack: String,
    /// Path relative to the pack root, using `/`. Empty for a pack-level record.
    pub file: String,
    pub line: Option<u32>,
    pub message: String,
    /// Set when the loader substituted something else, such as `"Stay"`.
    pub fallback: Option<String>,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        kind: DiagnosticKind,
        pack: impl Into<String>,
        file: impl Into<String>,
        line: Option<u32>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            kind,
            pack: pack.into(),
            file: file.into(),
            line,
            message: message.into(),
            fallback: None,
        }
    }

    pub fn with_fallback(mut self, fallback: impl Into<String>) -> Self {
        self.fallback = Some(fallback.into());
        self
    }

    /// True when this record is the unknown-class Stay substitution.
    pub fn is_stay_fallback(&self) -> bool {
        self.fallback.as_deref() == Some("Stay")
    }
}
