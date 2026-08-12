//! Format-neutral source-to-audio timing.

use crate::book::SourceRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTimeline {
    pub duration_ms: u64,
    pub cues: Vec<AudioCue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioCue {
    pub id: String,
    pub kind: CueKind,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    pub source_range: Option<SourceRange>,
    pub section_id: Option<String>,
    pub timing: TimingGranularity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CueKind {
    Book,
    Part,
    Chapter,
    Section,
    Page { label: String },
    Paragraph,
    Sentence,
    Footnote,
    Figure,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingGranularity {
    /// Exact boundary of one or more synthesized provider segments.
    SegmentBoundary,
}
