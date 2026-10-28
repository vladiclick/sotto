// models.rs — the speech models Sotto can download and switch between.
//
// All are whisper.cpp GGML files from huggingface.co/ggerganov/whisper.cpp,
// pinned by exact byte size and SHA-256 so a truncated or tampered download
// is never loaded. Quantized variants keep RAM and disk small with a
// negligible accuracy cost.

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    pub id: &'static str,
    pub file: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub size: u64,
    #[serde(skip)]
    pub sha256: &'static str,
    /// 1 (slowest) … 5 (fastest), relative on Apple Silicon.
    pub speed: u8,
    /// 1 (roughest) … 5 (most accurate).
    pub accuracy: u8,
    /// Trained on English only: faster and sharper for English, no other
    /// languages, no translation.
    pub english_only: bool,
    /// Can translate speech in other languages into English text.
    /// (large-v3-turbo was trained without the translation task.)
    pub translates: bool,
}

pub const DEFAULT_MODEL: &str = "large-v3-turbo";

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "tiny",
        file: "ggml-tiny-q5_1.bin",
        name: "Tiny",
        summary: "Instant and tiny. Fine for short notes in clear English.",
        size: 32_152_673,
        sha256: "818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7",
        speed: 5,
        accuracy: 1,
        english_only: false,
        translates: true,
    },
    ModelSpec {
        id: "base",
        file: "ggml-base-q5_1.bin",
        name: "Base",
        summary: "Very fast, noticeably better than Tiny.",
        size: 59_707_625,
        sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
        speed: 5,
        accuracy: 2,
        english_only: false,
        translates: true,
    },
    ModelSpec {
        id: "small",
        file: "ggml-small-q5_1.bin",
        name: "Small",
        summary: "Good everyday accuracy on older Macs.",
        size: 190_085_487,
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        speed: 4,
        accuracy: 3,
        english_only: false,
        translates: true,
    },
    ModelSpec {
        id: "small.en",
        file: "ggml-small.en-q5_1.bin",
        name: "Small · English",
        summary: "English-only Small: sharper English at the same speed.",
        size: 190_098_681,
        sha256: "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30",
        speed: 4,
        accuracy: 3,
        english_only: true,
        translates: false,
    },
    ModelSpec {
        id: "medium",
        file: "ggml-medium-q5_0.bin",
        name: "Medium",
        summary: "Strong accuracy with translation; slower than Turbo.",
        size: 539_212_467,
        sha256: "19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f",
        speed: 2,
        accuracy: 4,
        english_only: false,
        translates: true,
    },
    ModelSpec {
        id: "large-v3-turbo",
        file: "ggml-large-v3-turbo-q5_0.bin",
        name: "Large v3 Turbo",
        summary: "The default: near-Large accuracy at a fraction of the time.",
        size: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        speed: 3,
        accuracy: 5,
        english_only: false,
        translates: false,
    },
    ModelSpec {
        id: "large-v3-turbo-q8",
        file: "ggml-large-v3-turbo-q8_0.bin",
        name: "Large v3 Turbo · HQ",
        summary: "8-bit Turbo: a touch more accurate, more RAM.",
        size: 874_188_075,
        sha256: "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
        speed: 3,
        accuracy: 5,
        english_only: false,
        translates: false,
    },
    ModelSpec {
        id: "large-v3",
        file: "ggml-large-v3-q5_0.bin",
        name: "Large v3",
        summary: "The most accurate model, with translation. Slowest.",
        size: 1_081_140_203,
        sha256: "d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1",
        speed: 1,
        accuracy: 5,
        english_only: false,
        translates: true,
    },
];

pub fn find(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id)
}

pub fn url(spec: &ModelSpec) -> String {
    format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        spec.file
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        assert!(find(DEFAULT_MODEL).is_some());
        for (i, m) in MODELS.iter().enumerate() {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(m.sha256.chars().all(|c| c.is_ascii_hexdigit()), "{}", m.id);
            assert!(m.file.starts_with("ggml-") && m.file.ends_with(".bin"));
            assert!((1..=5).contains(&m.speed) && (1..=5).contains(&m.accuracy));
            assert!(!(m.english_only && m.translates), "{}", m.id);
            assert!(
                MODELS[i + 1..]
                    .iter()
                    .all(|o| o.id != m.id && o.file != m.file),
                "duplicate {}",
                m.id
            );
        }
    }
}
