//! Kokoro v1.0 English preset voices.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

pub const DEFAULT_VOICE: &str = "af_heart";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceInfo {
    pub name: &'static str,
    pub speaker_id: i32,
}

pub const ENGLISH_VOICES: [VoiceInfo; 28] = [
    voice("af_alloy", 0),
    voice("af_aoede", 1),
    voice("af_bella", 2),
    voice("af_heart", 3),
    voice("af_jessica", 4),
    voice("af_kore", 5),
    voice("af_nicole", 6),
    voice("af_nova", 7),
    voice("af_river", 8),
    voice("af_sarah", 9),
    voice("af_sky", 10),
    voice("am_adam", 11),
    voice("am_echo", 12),
    voice("am_eric", 13),
    voice("am_fenrir", 14),
    voice("am_liam", 15),
    voice("am_michael", 16),
    voice("am_onyx", 17),
    voice("am_puck", 18),
    voice("am_santa", 19),
    voice("bf_alice", 20),
    voice("bf_emma", 21),
    voice("bf_isabella", 22),
    voice("bf_lily", 23),
    voice("bm_daniel", 24),
    voice("bm_fable", 25),
    voice("bm_george", 26),
    voice("bm_lewis", 27),
];

const fn voice(name: &'static str, speaker_id: i32) -> VoiceInfo {
    VoiceInfo { name, speaker_id }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Voice(VoiceInfo);

impl Voice {
    pub const fn name(self) -> &'static str {
        self.0.name
    }

    pub const fn speaker_id(self) -> i32 {
        self.0.speaker_id
    }
}

impl FromStr for Voice {
    type Err = VoiceError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        ENGLISH_VOICES
            .iter()
            .find(|voice| voice.name == name)
            .copied()
            .map(Self)
            .ok_or_else(|| VoiceError(name.to_owned()))
    }
}

impl fmt::Display for Voice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("unknown English voice '{0}'; run `kokoro-book voices`")]
pub struct VoiceError(String);
