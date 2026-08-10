//! Kokoro v1.0 English preset voices.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

pub const DEFAULT_VOICE: &str = "af_heart";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceInfo {
    pub name: &'static str,
    pub sha256: &'static str,
}

pub const ENGLISH_VOICES: [VoiceInfo; 28] = [
    voice(
        "af_alloy",
        "5bb848d02ade7e37981809acad52a1761ef7a586ff9f30d02d65fd71c4af95f9",
    ),
    voice(
        "af_aoede",
        "23809148777f2a2378983dd856bc14b9c261018279f916f98c23d86e844409a5",
    ),
    voice(
        "af_bella",
        "112d310468cbb3cf23404d3d0b50ad3adf017b87bf38bf9edd15f4ad572df6a3",
    ),
    voice(
        "af_heart",
        "2c1c733b0e6576c810e268d3e440c21dea4e0f0131a3ba4cfc98d7fe6136d094",
    ),
    voice(
        "af_jessica",
        "c358448e4277b79e8b13b92033711660a1a2205c3940c2dfb16698b99fed58a8",
    ),
    voice(
        "af_kore",
        "c491174280cb1ad25210a842f2f34b46a9ef904ec6f6a8e784839531795fa278",
    ),
    voice(
        "af_nicole",
        "574656386022c81a029e9a72558191925f44c3de2dad2fa2e45751938557d062",
    ),
    voice(
        "af_nova",
        "242b9a0a01eac1ac2865c69fc617a756b20d86df82d5fae3970533e2312ca50e",
    ),
    voice(
        "af_river",
        "82c866b0b976d50e82cbd781ac7bc771471ce5bd21decf05ab92812a08fb1c04",
    ),
    voice(
        "af_sarah",
        "4940072182542f54c1035d1daf4c1cf3136ca9baa9ac57c8e006b4befcc50be6",
    ),
    voice(
        "af_sky",
        "957af332330db8e9bd7f9dc449475a946cb0d7d689afef64b91007bbbf20eaa0",
    ),
    voice(
        "am_adam",
        "a4f60a3b9c20353c2604a17485ba53260502a758681a84d41e8af53cc559d929",
    ),
    voice(
        "am_echo",
        "031fc608a900332c4e1a29bd0884f5d0e84bd0348261fa79981e5cbd138c950d",
    ),
    voice(
        "am_eric",
        "1fb4a61dcee1f114f90886ecf29bc2feed05e29eed9caa6ddb109f1934d73274",
    ),
    voice(
        "am_fenrir",
        "9abed964b906c4cae6f404d9849e76260689aea862bc6ca85fc3f5207ba96538",
    ),
    voice(
        "am_liam",
        "66b65a96e16c3d91035a6e9019d9986ed524d27ce35b487270cdf61c99e3ebad",
    ),
    voice(
        "am_michael",
        "3940147ded35deba0bb52e8132f89b719298e0520258c34584358aa5a24da2ea",
    ),
    voice(
        "am_onyx",
        "b5d6132a5747648d98c82c9c4aaa9cf52d7230e63e403c1cb9c12858446ca5f5",
    ),
    voice(
        "am_puck",
        "9a8c2e56413bd2063f814cb4c3885fc425876157369117c3f8258d03c8a9ad89",
    ),
    voice(
        "am_santa",
        "d1f433b57ffccf105ea9e434ea19af6c2a8a7916ba6d1a73c34f0046bd226084",
    ),
    voice(
        "bf_alice",
        "9c77e390d93d9db7c4a7526c3b1f393290a2be46f233b89a00b8188e850c20a8",
    ),
    voice(
        "bf_emma",
        "8878a75a6661305849eeb1d6293a7177250193616e161b4c3100636434dfe69f",
    ),
    voice(
        "bf_isabella",
        "f7b6076f025649699fcfed1a6debf13049a87afdc7aafc8c72b7d81246db6ead",
    ),
    voice(
        "bf_lily",
        "ee77a419046a765420ac82cb46e8b8cf5754a0b9d20c340fece1d4b18be7ecdb",
    ),
    voice(
        "bm_daniel",
        "b195dec592ee024f57ddc5bf481464596082ba60998a2a295eba90bfc1064f4b",
    ),
    voice(
        "bm_fable",
        "9fa80184e96d016a744bc13b0b2e7695e55d6b855556fa003325cb1e5ebf2c2b",
    ),
    voice(
        "bm_george",
        "a3d9b8995cbbe5536f954b6be2a0f1f312f077118ba0d4d2178fc41dc8306672",
    ),
    voice(
        "bm_lewis",
        "e1e68013c21a141efe527aaec561e1174c2f5a6951b3bcecc8396adab315b247",
    ),
];

const fn voice(name: &'static str, sha256: &'static str) -> VoiceInfo {
    VoiceInfo { name, sha256 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Voice(VoiceInfo);

impl Voice {
    pub const fn name(self) -> &'static str {
        self.0.name
    }

    pub const fn sha256(self) -> &'static str {
        self.0.sha256
    }

    pub fn is_british(self) -> bool {
        self.name().starts_with('b')
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
