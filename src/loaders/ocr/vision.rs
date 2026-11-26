use super::engine::{line, EngineError, OcrLine, PageImage};
use objc2::rc::{autoreleasepool, Retained};
use objc2::AllocAnyThread;
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    kCVPixelFormatType_32BGRA, kCVReturnSuccess, CVPixelBuffer, CVPixelBufferCreate,
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetDataSize,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSArray, NSDictionary};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRequest,
    VNRequestTextRecognitionLevel,
};

pub struct VisionEngine {
    minimum_confidence: f32,
}

impl VisionEngine {
    pub fn new(minimum_confidence: f32) -> Self {
        Self { minimum_confidence }
    }

    pub fn recognize(&self, page: &PageImage) -> Result<Vec<OcrLine>, EngineError> {
        autoreleasepool(|_| recognize(&page.rgba, page.width, page.height, self.minimum_confidence))
            .map_err(EngineError::Vision)
    }
}

fn text_request() -> Retained<VNRecognizeTextRequest> {
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    request
}

fn recognize(
    rgba: &[u8],
    width: u32,
    height: u32,
    minimum_confidence: f32,
) -> Result<Vec<OcrLine>, String> {
    let image = pixel_buffer(rgba, width, height)
        .ok_or_else(|| "CoreVideo could not hold the page".to_string())?;
    let request = text_request();
    let handler = unsafe {
        VNImageRequestHandler::initWithCVPixelBuffer_options(
            VNImageRequestHandler::alloc(),
            &image,
            &NSDictionary::new(),
        )
    };
    let as_request: Retained<VNRequest> =
        Retained::into_super(Retained::into_super(request.clone()));
    handler
        .performRequests_error(&NSArray::from_retained_slice(&[as_request]))
        .map_err(|err| err.localizedDescription().to_string())?;
    let Some(observations) = request.results() else {
        return Ok(Vec::new());
    };
    Ok(observations
        .iter()
        .filter_map(|observation| observation_line(&observation, width, height, minimum_confidence))
        .collect())
}

fn observation_line(
    observation: &VNRecognizedTextObservation,
    width: u32,
    height: u32,
    minimum_confidence: f32,
) -> Option<OcrLine> {
    let candidate = observation.topCandidates(1).firstObject()?;
    let corners = unsafe {
        [
            observation.topLeft(),
            observation.topRight(),
            observation.bottomRight(),
            observation.bottomLeft(),
        ]
    };
    let (w, h) = (width as f32, height as f32);
    line(
        &candidate.string().to_string(),
        candidate.confidence(),
        corners
            .iter()
            .map(|corner| (corner.x as f32 * w, (1.0 - corner.y as f32) * h)),
        width,
        height,
        minimum_confidence,
    )
}

fn pixel_buffer(rgba: &[u8], width: u32, height: u32) -> Option<CFRetained<CVPixelBuffer>> {
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    if rgba.len() != width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    let mut created: *mut CVPixelBuffer = std::ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            width,
            height,
            kCVPixelFormatType_32BGRA,
            None,
            std::ptr::NonNull::from(&mut created),
        )
    };
    let buffer = unsafe { CFRetained::from_raw(std::ptr::NonNull::new(created)?) };
    if status != kCVReturnSuccess
        || unsafe { CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) }
            != kCVReturnSuccess
    {
        return None;
    }
    let stride = CVPixelBufferGetBytesPerRow(&buffer);
    let base = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
    let filled = !base.is_null()
        && stride >= width * 4
        && stride
            .checked_mul(height)
            .is_some_and(|size| size <= CVPixelBufferGetDataSize(&buffer));
    if filled {
        let bgra = unsafe { std::slice::from_raw_parts_mut(base, stride * height) };
        for (source, target) in rgba
            .chunks_exact(width * 4)
            .zip(bgra.chunks_exact_mut(stride))
        {
            for (rgba, bgra) in source
                .as_chunks::<4>()
                .0
                .iter()
                .zip(target.as_chunks_mut::<4>().0)
            {
                *bgra = [rgba[2], rgba[1], rgba[0], u8::MAX];
            }
        }
    }
    unsafe { CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) };
    filled.then_some(buffer)
}
