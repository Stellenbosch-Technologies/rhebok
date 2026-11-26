use super::engine::{line, EngineError, EngineSettings, OcrLine, PageImage};
use crate::config::ModelDownloads;
use oar_ocr::oarocr::{OAROCRBuilder, OAROCR};
use std::path::{Path, PathBuf};

pub const MODEL_FILES: [&str; 3] = [
    "pp-ocrv6_small_det.onnx",
    "pp-ocrv6_small_rec.onnx",
    "ppocrv6_dict.txt",
];

pub struct OarEngine {
    ocr: OAROCR,
    minimum_confidence: f32,
}

impl OarEngine {
    pub fn new(settings: &EngineSettings) -> Result<Self, EngineError> {
        let [detection, recognition, dictionary] = model_paths(settings)?;
        let ocr = OAROCRBuilder::new(detection, recognition, dictionary).build()?;
        Ok(Self {
            ocr,
            minimum_confidence: settings.minimum_confidence,
        })
    }

    pub fn recognize(&self, page: &PageImage) -> Result<Vec<OcrLine>, EngineError> {
        let rgb: Vec<u8> = page
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|rgba| [rgba[0], rgba[1], rgba[2]])
            .collect();
        let image =
            image::RgbImage::from_raw(page.width, page.height, rgb).ok_or(EngineError::Image)?;
        let Some(result) = self.ocr.predict(vec![image])?.into_iter().next() else {
            return Ok(Vec::new());
        };
        Ok(result
            .text_regions
            .iter()
            .filter_map(|region| {
                line(
                    region.text.as_deref()?,
                    region.confidence?,
                    region
                        .bounding_box
                        .points
                        .iter()
                        .map(|point| (point.x, point.y)),
                    page.width,
                    page.height,
                    self.minimum_confidence,
                )
            })
            .collect())
    }
}

fn model_paths(settings: &EngineSettings) -> Result<[PathBuf; 3], EngineError> {
    let may_download =
        settings.model_dir.is_none() && settings.downloads == ModelDownloads::IfMissing;
    let dir = settings
        .model_dir
        .clone()
        .unwrap_or_else(oar_ocr::core::download::cache_dir);
    let mut paths = MODEL_FILES.map(|name| dir.join(name));
    for (path, name) in paths.iter_mut().zip(MODEL_FILES) {
        *path = present_or_fetched(path, name, may_download)?;
    }
    Ok(paths)
}

fn present_or_fetched(path: &Path, name: &str, may_download: bool) -> Result<PathBuf, EngineError> {
    if may_download {
        return Ok(oar_ocr::core::download::fetch(name)?);
    }
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    Err(EngineError::MissingModel(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(model_dir: Option<PathBuf>, downloads: ModelDownloads) -> EngineSettings {
        EngineSettings {
            model_dir,
            downloads,
            minimum_confidence: 0.5,
            command: None,
            url: None,
            token: None,
            timeout: std::time::Duration::from_secs(5),
        }
    }

    #[test]
    fn a_model_folder_is_used_as_it_is_and_never_downloaded_into() {
        let dir = tempfile::tempdir().unwrap();
        let missing = model_paths(&settings(
            Some(dir.path().to_path_buf()),
            ModelDownloads::IfMissing,
        ));
        assert!(
            matches!(missing, Err(EngineError::MissingModel(ref p)) if p == &dir.path().join(MODEL_FILES[0])),
            "{missing:?}"
        );
        for name in MODEL_FILES {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let found = model_paths(&settings(
            Some(dir.path().to_path_buf()),
            ModelDownloads::IfMissing,
        ))
        .unwrap();
        assert_eq!(found, MODEL_FILES.map(|name| dir.path().join(name)));
    }

    #[test]
    fn offline_refuses_a_file_that_is_not_already_there() {
        let missing = Path::new("/definitely/not/here/pp-ocrv6_small_det.onnx");
        assert!(matches!(
            present_or_fetched(missing, MODEL_FILES[0], false),
            Err(EngineError::MissingModel(_))
        ));
    }
}
