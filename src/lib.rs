//! Core library for the `kokoro-book` CLI.

#![forbid(unsafe_code)]

pub mod audio;
pub mod book;
pub mod chunk;
pub mod cli;
pub mod input;
pub mod model;
mod phoneme;
pub mod pipeline;
pub mod tts;
mod vocab;
pub mod voice;
pub mod worker;
