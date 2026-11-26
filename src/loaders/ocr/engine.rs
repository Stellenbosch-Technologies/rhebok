use super::EngineKind;
use crate::config::ModelDownloads;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    pub text: String,
    pub confidence: f32,
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineSettings {
    pub model_dir: Option<PathBuf>,
    pub downloads: ModelDownloads,
    pub minimum_confidence: f32,
    pub command: Option<Vec<String>>,
    pub url: Option<String>,
    pub token: Option<String>,
    pub timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Apple Vision is only available on macOS; set `ocr_engine = \"oar\"` under [pdf]")]
    VisionUnavailable,
    #[error("Apple Vision could not read the page: {0}")]
    Vision(String),
    #[error("the page image has a size the OCR engine cannot take")]
    Image,
    #[error("minimum OCR confidence must be between 0 and 1, got {0}")]
    InvalidConfidence(f32),
    #[error(
        "the OCR model file {} is missing and model downloads are off",
        .0.display()
    )]
    MissingModel(PathBuf),
    #[error(transparent)]
    Oar(#[from] oar_ocr::core::OCRError),
    #[error("{0}")]
    NotConfigured(&'static str),
    #[error("the OCR command could not be started: {0}")]
    CommandMissing(String),
    #[error("{0}")]
    Endpoint(String),
    #[error("{0}")]
    Page(String),
}

impl EngineError {
    pub fn spoils_the_run(&self) -> bool {
        !matches!(self, EngineError::Page(_))
    }
}

pub enum Engine {
    #[cfg(target_os = "macos")]
    Vision(super::vision::VisionEngine),
    Oar(Box<super::oar::OarEngine>),
    Command(super::external::CommandEngine),
    Http(super::external::HttpEngine),
}

impl Engine {
    pub fn recognize(&self, page: &PageImage) -> Result<Vec<OcrLine>, EngineError> {
        on_a_plain_thread(|| match self {
            #[cfg(target_os = "macos")]
            Engine::Vision(engine) => engine.recognize(page),
            Engine::Oar(engine) => engine.recognize(page),
            Engine::Command(engine) => engine.recognize(page),
            Engine::Http(engine) => engine.recognize(page),
        })
    }
}

fn on_a_plain_thread<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    std::thread::scope(|scope| {
        scope
            .spawn(work)
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

static ENGINE: Mutex<Option<(EngineKind, EngineSettings, Arc<Engine>)>> = Mutex::new(None);

pub fn engine_for(kind: EngineKind, settings: &EngineSettings) -> Result<Arc<Engine>, EngineError> {
    let confidence = settings.minimum_confidence;
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(EngineError::InvalidConfidence(confidence));
    }
    let mut cached = ENGINE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((cached_kind, cached_settings, engine)) = cached.as_ref() {
        if *cached_kind == kind && cached_settings == settings {
            return Ok(Arc::clone(engine));
        }
    }
    let engine = Arc::new(build(kind, settings)?);
    *cached = Some((kind, settings.clone(), Arc::clone(&engine)));
    Ok(engine)
}

fn build(kind: EngineKind, settings: &EngineSettings) -> Result<Engine, EngineError> {
    match kind {
        EngineKind::Vision => vision_engine(settings),
        EngineKind::Oar => Ok(Engine::Oar(Box::new(super::oar::OarEngine::new(settings)?))),
        EngineKind::Command => Ok(Engine::Command(super::external::CommandEngine::new(
            settings.command.as_deref(),
            settings.timeout,
            settings.minimum_confidence,
        )?)),
        EngineKind::Http => Ok(Engine::Http(super::external::HttpEngine::new(
            settings.url.as_deref(),
            settings.token.clone(),
            settings.timeout,
            settings.minimum_confidence,
        )?)),
    }
}

#[cfg(target_os = "macos")]
fn vision_engine(settings: &EngineSettings) -> Result<Engine, EngineError> {
    Ok(Engine::Vision(super::vision::VisionEngine::new(
        settings.minimum_confidence,
    )))
}

#[cfg(not(target_os = "macos"))]
fn vision_engine(_settings: &EngineSettings) -> Result<Engine, EngineError> {
    Err(EngineError::VisionUnavailable)
}

pub(super) fn line(
    text: &str,
    confidence: f32,
    corners: impl IntoIterator<Item = (f32, f32)>,
    width: u32,
    height: u32,
    minimum_confidence: f32,
) -> Option<OcrLine> {
    let text = text.trim();
    if text.is_empty() || !confidence.is_finite() {
        return None;
    }
    let confidence = confidence.clamp(0.0, 1.0);
    if confidence < minimum_confidence {
        return None;
    }
    let (mut left, mut top, mut right, mut bottom) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for (x, y) in corners {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (x, y) = (x.clamp(0.0, width as f32), y.clamp(0.0, height as f32));
        left = left.min(x);
        right = right.max(x);
        top = top.min(y);
        bottom = bottom.max(y);
    }
    (right > left && bottom > top).then(|| OcrLine {
        text: text.to_string(),
        confidence,
        left,
        top,
        right,
        bottom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognition_never_runs_on_a_rayon_worker_even_when_called_from_one() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let (caller, worker) = pool.install(|| {
            (
                rayon::current_thread_index(),
                on_a_plain_thread(rayon::current_thread_index),
            )
        });
        assert!(caller.is_some(), "the test must call from a rayon worker");
        assert_eq!(worker, None);
    }

    #[test]
    fn a_panic_in_recognition_reaches_the_caller() {
        let caught = std::panic::catch_unwind(|| on_a_plain_thread(|| panic!("page")));
        assert!(caught.is_err());
    }

    #[test]
    fn a_line_is_the_box_around_its_corners_clamped_to_the_page() {
        let line = line(
            " word ",
            0.9,
            [(5.0, 2.0), (-3.0, 4.0), (40.0, 20.0)],
            30,
            10,
            0.5,
        )
        .unwrap();
        assert_eq!(line.text, "word");
        assert_eq!(
            (line.left, line.top, line.right, line.bottom),
            (0.0, 2.0, 30.0, 10.0)
        );
    }

    #[test]
    fn a_line_below_the_floor_or_without_text_or_area_is_dropped() {
        let corners = [(0.0, 0.0), (10.0, 5.0)];
        assert!(line("word", 0.49, corners, 30, 10, 0.5).is_none());
        assert!(line("   ", 0.99, corners, 30, 10, 0.5).is_none());
        assert!(line("word", f32::NAN, corners, 30, 10, 0.0).is_none());
        assert!(line("word", 0.9, [(3.0, 0.0), (3.0, 5.0)], 30, 10, 0.5).is_none());
        assert_eq!(
            line("word", 1.7, corners, 30, 10, 0.5).unwrap().confidence,
            1.0
        );
    }

    fn settings() -> EngineSettings {
        EngineSettings {
            model_dir: None,
            downloads: ModelDownloads::Offline,
            minimum_confidence: 0.5,
            command: None,
            url: None,
            token: None,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn only_a_page_failure_leaves_ocr_on_for_the_rest_of_the_run() {
        assert!(!EngineError::Page("timed out".into()).spoils_the_run());
        for fatal in [
            EngineError::NotConfigured("no command"),
            EngineError::CommandMissing("tesseract".into()),
            EngineError::Endpoint("refused".into()),
            EngineError::VisionUnavailable,
            EngineError::Image,
        ] {
            assert!(fatal.spoils_the_run(), "{fatal}");
        }
    }

    #[test]
    fn an_external_engine_without_its_setting_refuses_to_build() {
        for kind in [EngineKind::Command, EngineKind::Http] {
            assert!(matches!(
                engine_for(kind, &settings()),
                Err(EngineError::NotConfigured(_))
            ));
        }
    }

    #[test]
    fn a_confidence_floor_outside_zero_to_one_refuses_to_build_an_engine() {
        for floor in [-0.1, 1.5, f32::NAN] {
            let settings = EngineSettings {
                minimum_confidence: floor,
                ..settings()
            };
            assert!(matches!(
                engine_for(EngineKind::Oar, &settings),
                Err(EngineError::InvalidConfidence(_))
            ));
        }
    }
}
