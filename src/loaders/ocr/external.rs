use super::engine::{line, EngineError, OcrLine, PageImage};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const IMAGE_PLACEHOLDER: &str = "{image}";

pub struct CommandEngine {
    argv: Vec<String>,
    timeout: Duration,
    minimum_confidence: f32,
}

impl CommandEngine {
    pub fn new(
        argv: Option<&[String]>,
        timeout: Duration,
        minimum_confidence: f32,
    ) -> Result<Self, EngineError> {
        match argv {
            Some(argv)
                if argv
                    .first()
                    .is_some_and(|program| !program.trim().is_empty()) =>
            {
                Ok(Self {
                    argv: argv.to_vec(),
                    timeout,
                    minimum_confidence,
                })
            }
            _ => Err(EngineError::NotConfigured(
                "ocr_engine = \"command\" needs ocr_command under [pdf]",
            )),
        }
    }

    pub fn recognize(&self, page: &PageImage) -> Result<Vec<OcrLine>, EngineError> {
        let png = png(page)?;
        let output = if self.argv.iter().any(|arg| arg.contains(IMAGE_PLACEHOLDER)) {
            let file = TempImage::write(&png)?;
            let argv: Vec<String> = self
                .argv
                .iter()
                .map(|arg| arg.replace(IMAGE_PLACEHOLDER, &file.0.to_string_lossy()))
                .collect();
            run(&argv, None, self.timeout)?
        } else {
            run(&self.argv, Some(png), self.timeout)?
        };
        Ok(parse(
            &output,
            page.width,
            page.height,
            self.minimum_confidence,
        ))
    }
}

pub struct HttpEngine {
    url: String,
    token: Option<String>,
    client: reqwest::blocking::Client,
    minimum_confidence: f32,
}

impl HttpEngine {
    pub fn new(
        url: Option<&str>,
        token: Option<String>,
        timeout: Duration,
        minimum_confidence: f32,
    ) -> Result<Self, EngineError> {
        let url = url
            .map(str::trim)
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .ok_or(EngineError::NotConfigured(
                "ocr_engine = \"http\" needs ocr_url under [pdf], starting with http:// or https://",
            ))?;
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| {
                EngineError::Endpoint(format!("the OCR endpoint client could not start: {e}"))
            })?;
        Ok(Self {
            url: url.to_string(),
            token,
            client,
            minimum_confidence,
        })
    }

    pub fn recognize(&self, page: &PageImage) -> Result<Vec<OcrLine>, EngineError> {
        let mut request = self
            .client
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "image/png")
            .body(png(page)?);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().map_err(|e| {
            if e.is_timeout() {
                EngineError::Page(format!("the OCR endpoint did not answer in time: {e}"))
            } else {
                EngineError::Endpoint(format!("the OCR endpoint could not be reached: {e}"))
            }
        })?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(EngineError::Endpoint(format!(
                "the OCR endpoint refused the token ({status}); set RHEBOK_OCR_TOKEN"
            )));
        }
        if !status.is_success() {
            return Err(EngineError::Page(format!(
                "the OCR endpoint answered {status}"
            )));
        }
        let body = response
            .text()
            .map_err(|e| EngineError::Page(format!("reading the OCR endpoint's answer: {e}")))?;
        Ok(parse(
            &body,
            page.width,
            page.height,
            self.minimum_confidence,
        ))
    }
}

fn png(page: &PageImage) -> Result<Vec<u8>, EngineError> {
    let image = image::RgbaImage::from_raw(page.width, page.height, page.rgba.clone())
        .ok_or(EngineError::Image)?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|_| EngineError::Image)?;
    Ok(bytes.into_inner())
}

struct TempImage(PathBuf);

impl TempImage {
    fn write(png: &[u8]) -> Result<Self, EngineError> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rhebok-ocr-{}-{}.png",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, png)
            .map_err(|e| EngineError::Page(format!("writing the page image: {e}")))?;
        Ok(Self(path))
    }
}

impl Drop for TempImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(argv: &[String], stdin: Option<Vec<u8>>, timeout: Duration) -> Result<String, EngineError> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| EngineError::CommandMissing(format!("{}: {e}", argv[0])))?;
    let writer = stdin.zip(child.stdin.take()).map(|(bytes, mut pipe)| {
        std::thread::spawn(move || {
            let _ = pipe.write_all(&bytes);
        })
    });
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(EngineError::Page(format!(
                    "{} did not finish within {} seconds",
                    argv[0],
                    timeout.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(EngineError::Page(format!("waiting for {}: {e}", argv[0]))),
        }
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        let said = String::from_utf8_lossy(&stderr);
        let said = said.trim();
        let tail: String = said
            .chars()
            .rev()
            .take(300)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(EngineError::Page(format!(
            "{} failed ({status}): {tail}",
            argv[0]
        )));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

#[derive(serde::Deserialize)]
struct Reported {
    text: String,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default, rename = "box")]
    bounds: Option<[f32; 4]>,
}

pub fn parse(output: &str, width: u32, height: u32, minimum_confidence: f32) -> Vec<OcrLine> {
    let reported: Option<Vec<Reported>> = if output.trim_start().starts_with('[') {
        serde_json::from_str(output.trim()).ok()
    } else {
        output
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).ok())
            .collect()
    };
    let reported = reported
        .filter(|lines| !lines.is_empty())
        .unwrap_or_else(|| {
            output
                .lines()
                .map(|text| Reported {
                    text: text.to_string(),
                    confidence: None,
                    bounds: None,
                })
                .collect()
        });
    let boxed = reported.iter().all(|r| r.bounds.is_some());
    let row = height as f32 / reported.len().max(1) as f32;
    reported
        .into_iter()
        .enumerate()
        .filter_map(|(index, r)| {
            let [left, top, right, bottom] = match r.bounds {
                Some(bounds) if boxed => bounds,
                _ => {
                    let top = index as f32 * row;
                    [0.0, top, width as f32, top + row]
                }
            };
            line(
                &r.text,
                r.confidence.unwrap_or(1.0),
                [(left, top), (right, bottom)],
                width,
                height,
                minimum_confidence,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> PageImage {
        PageImage {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        }
    }

    fn texts(lines: &[OcrLine]) -> Vec<&str> {
        lines.iter().map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn plain_text_keeps_its_line_order_and_drops_blank_lines() {
        let lines = parse("first line\n\n  second line  \nthird\n", 100, 300, 0.5);
        assert_eq!(texts(&lines), ["first line", "second line", "third"]);
        assert_eq!(
            super::super::page_text(&lines, 100.0),
            "first line\nsecond line\nthird"
        );
    }

    #[test]
    fn json_lines_carry_boxes_and_confidence() {
        let output = r#"{"text": "right", "confidence": 0.9, "box": [60, 10, 100, 20]}
{"text": "left", "confidence": 0.95, "box": [0, 10, 50, 20]}
{"text": "faint", "confidence": 0.2, "box": [0, 40, 50, 50]}"#;
        let lines = parse(output, 100, 100, 0.5);
        assert_eq!(texts(&lines), ["right", "left"]);
        assert_eq!(super::super::page_text(&lines, 100.0), "left right");
    }

    #[test]
    fn a_json_array_is_read_like_json_lines() {
        let lines = parse(r#"[{"text": "one"}, {"text": "two"}]"#, 10, 10, 0.5);
        assert_eq!(texts(&lines), ["one", "two"]);
    }

    #[test]
    fn text_that_only_looks_like_json_is_read_as_text() {
        let lines = parse("{not json}\nplain", 10, 10, 0.5);
        assert_eq!(texts(&lines), ["{not json}", "plain"]);
    }

    #[test]
    fn a_command_gets_the_page_as_a_file_and_its_output_is_read() {
        let engine = CommandEngine::new(
            Some(&[
                "sh".into(),
                "-c".into(),
                "test -s \"$0\" && echo read".into(),
                "{image}".into(),
            ]),
            Duration::from_secs(10),
            0.5,
        )
        .unwrap();
        assert_eq!(texts(&engine.recognize(&page()).unwrap()), ["read"]);
    }

    #[test]
    fn without_a_placeholder_the_page_goes_to_standard_input() {
        let engine = CommandEngine::new(
            Some(&["sh".into(), "-c".into(), "head -c 4 | tail -c 3".into()]),
            Duration::from_secs(10),
            0.5,
        )
        .unwrap();
        assert_eq!(texts(&engine.recognize(&page()).unwrap()), ["PNG"]);
    }

    #[test]
    fn the_temporary_page_image_is_removed_after_the_command() {
        let engine = CommandEngine::new(
            Some(&[
                "sh".into(),
                "-c".into(),
                "echo \"$0\"".into(),
                "{image}".into(),
            ]),
            Duration::from_secs(10),
            0.0,
        )
        .unwrap();
        let path = engine.recognize(&page()).unwrap().remove(0).text;
        assert!(path.ends_with(".png"), "{path}");
        assert!(!std::path::Path::new(&path).exists());
    }

    #[test]
    fn a_failing_or_slow_command_fails_its_page_and_not_the_run() {
        let failing = CommandEngine::new(
            Some(&["sh".into(), "-c".into(), "echo bad page >&2; exit 3".into()]),
            Duration::from_secs(10),
            0.5,
        )
        .unwrap();
        let err = failing.recognize(&page()).unwrap_err();
        assert!(!err.spoils_the_run(), "{err}");
        assert!(err.to_string().contains("bad page"), "{err}");

        let slow = CommandEngine::new(
            Some(&["sleep".into(), "5".into()]),
            Duration::from_millis(200),
            0.5,
        )
        .unwrap();
        let started = Instant::now();
        let err = slow.recognize(&page()).unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the command was not killed"
        );
        assert!(!err.spoils_the_run(), "{err}");
    }

    #[test]
    fn a_missing_program_or_setting_spoils_the_run() {
        let missing = CommandEngine::new(
            Some(&["rhebok-no-such-ocr-program".into()]),
            Duration::from_secs(10),
            0.5,
        )
        .unwrap();
        assert!(missing.recognize(&page()).unwrap_err().spoils_the_run());
        assert!(matches!(
            CommandEngine::new(None, Duration::from_secs(1), 0.5),
            Err(EngineError::NotConfigured(_))
        ));
        assert!(matches!(
            HttpEngine::new(Some("ftp://x"), None, Duration::from_secs(1), 0.5),
            Err(EngineError::NotConfigured(_))
        ));
    }

    fn serve_once(
        status: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/read", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length: usize = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (url, handle)
    }

    #[test]
    fn an_endpoint_gets_a_png_with_the_token_and_its_answer_is_read() {
        let (url, server) = serve_once("200 OK", r#"[{"text": "from the endpoint"}]"#);
        let engine = HttpEngine::new(
            Some(&url),
            Some("sekret".into()),
            Duration::from_secs(10),
            0.5,
        )
        .unwrap();
        let lines = engine.recognize(&page()).unwrap();
        assert_eq!(texts(&lines), ["from the endpoint"]);
        let request = server.join().unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(lower.starts_with("post /read "), "{request}");
        assert!(lower.contains("content-type: image/png"), "{request}");
        assert!(lower.contains("authorization: bearer sekret"), "{request}");
        assert!(request.contains("PNG"), "the body must be the page as PNG");
    }

    #[test]
    fn an_endpoint_error_fails_the_page_and_a_refused_token_fails_the_run() {
        let (url, server) = serve_once("500 Internal Server Error", "broken");
        let engine = HttpEngine::new(Some(&url), None, Duration::from_secs(10), 0.5).unwrap();
        assert!(!engine.recognize(&page()).unwrap_err().spoils_the_run());
        server.join().unwrap();

        let (url, server) = serve_once("401 Unauthorized", "no");
        let engine = HttpEngine::new(Some(&url), None, Duration::from_secs(10), 0.5).unwrap();
        assert!(engine.recognize(&page()).unwrap_err().spoils_the_run());
        server.join().unwrap();
    }
}
