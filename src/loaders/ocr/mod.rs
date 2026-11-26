use crate::config::OcrEngineChoice;

#[cfg(feature = "ocr")]
mod engine;
#[cfg(feature = "ocr")]
mod layout;
#[cfg(feature = "ocr")]
mod oar;
#[cfg(all(feature = "ocr", target_os = "macos"))]
mod vision;

#[cfg(feature = "ocr")]
pub use engine::{engine_for, Engine, EngineError, EngineSettings, OcrLine, PageImage};
#[cfg(feature = "ocr")]
pub use layout::{page_text, with_unseen_lines};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKind {
    Vision,
    Oar,
}

impl EngineKind {
    pub fn for_choice(choice: OcrEngineChoice) -> Self {
        Self::for_choice_on(choice, std::env::consts::OS)
    }

    pub fn for_choice_on(choice: OcrEngineChoice, os: &str) -> Self {
        match choice {
            OcrEngineChoice::Vision => EngineKind::Vision,
            OcrEngineChoice::Oar => EngineKind::Oar,
            OcrEngineChoice::Auto if os == "macos" => EngineKind::Vision,
            OcrEngineChoice::Auto => EngineKind::Oar,
        }
    }

    pub fn needs_onnx_runtime(self) -> bool {
        self == EngineKind::Oar
    }

    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Vision => "Apple Vision",
            EngineKind::Oar => "oar-ocr",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_picks_vision_on_macos_and_oar_everywhere_else() {
        assert_eq!(
            EngineKind::for_choice_on(OcrEngineChoice::Auto, "macos"),
            EngineKind::Vision
        );
        for os in ["linux", "windows", "freebsd"] {
            assert_eq!(
                EngineKind::for_choice_on(OcrEngineChoice::Auto, os),
                EngineKind::Oar,
                "{os}"
            );
        }
    }

    #[test]
    fn an_explicit_engine_is_kept_on_every_platform() {
        for os in ["macos", "linux"] {
            assert_eq!(
                EngineKind::for_choice_on(OcrEngineChoice::Vision, os),
                EngineKind::Vision
            );
            assert_eq!(
                EngineKind::for_choice_on(OcrEngineChoice::Oar, os),
                EngineKind::Oar
            );
        }
    }

    #[test]
    fn only_oar_needs_onnx_runtime() {
        assert!(EngineKind::Oar.needs_onnx_runtime());
        assert!(!EngineKind::Vision.needs_onnx_runtime());
    }
}
