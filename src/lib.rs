//! Core library for the `kokoro-book` CLI.

#![forbid(unsafe_code)]

pub mod audio;
pub mod book;
pub mod build;
pub mod chunk;
pub mod cli;
pub mod input;
pub mod m4b;
pub mod model;
pub mod narration;
pub(crate) mod phoneme;
pub mod pipeline;
pub mod preflight;
pub(crate) mod qwen;
pub mod sidecar;
pub mod synthesis;
pub mod timeline;
pub mod tts;
pub(crate) mod vocab;
pub mod voice;
pub mod worker;
