use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::scan::{file_stamp, is_image};

const MAX_FULL_BYTES: u64 = 120 * 1024 * 1024;

enum Kind {
    Thumb,
    Full,
}

struct MediaRequest {
    kind: Kind,
    edge: u32,
    path: PathBuf,
}

struct Pool {
    jobs: Mutex<VecDeque<Box<dyn FnOnce() + Send>>>,
    cv: Condvar,
}

impl Pool {
    fn start(&'static self) {
        for index in 0..3 {
            let pool = self;
            std::thread::Builder::new()
                .name(format!("photowall-thumb-{index}"))
                .spawn(move || loop {
                    let job = {
                        let mut queue = pool.jobs.lock().unwrap();
                        while queue.is_empty() {
                            queue = pool.cv.wait(queue).unwrap();
                        }
                        queue.pop_front().unwrap()
                    };
                    job();
                })
                .expect("spawn thumbnail worker");
        }
    }

    fn push(&'static self, job: Box<dyn FnOnce() + Send>) {
        let mut queue = self.jobs.lock().unwrap();
        queue.push_back(job);
        self.cv.notify_one();
    }
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    let pool = POOL.get_or_init(|| Pool {
        jobs: Mutex::new(VecDeque::new()),
        cv: Condvar::new(),
    });
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| pool.start());
    pool
}

pub fn fit_edge(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let max_edge = max_edge.max(1);
    let width = width.max(1);
    let height = height.max(1);
    if width <= max_edge && height <= max_edge {
        return (width, height);
    }
    let scale = f64::from(max_edge) / f64::from(width.max(height));
    let next_w = (f64::from(width) * scale).round().max(1.0) as u32;
    let next_h = (f64::from(height) * scale).round().max(1.0) as u32;
    (next_w.max(1), next_h.max(1))
}

fn parse_media_uri(raw: &str) -> Option<MediaRequest> {
    let path = if let Ok(url) = url::Url::parse(raw) {
        url.path().to_string()
    } else {
        raw.to_string()
    };
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() != 5 {
        return None;
    }
    let kind = match parts[0] {
        "thumb" => Kind::Thumb,
        "full" => Kind::Full,
        _ => return None,
    };
    let edge = parts[1].parse::<u32>().ok()?;
    let bytes = URL_SAFE_NO_PAD.decode(parts[4]).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    if text.is_empty() {
        return None;
    }
    Some(MediaRequest {
        kind,
        edge,
        path: PathBuf::from(text),
    })
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn response(status: u16, mime: &str, cache: &str, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    let length = body.len().to_string();
    tauri::http::Response::builder()
        .status(status)
        .header(tauri::http::header::CONTENT_TYPE, mime)
        .header(tauri::http::header::CONTENT_LENGTH, length)
        .header(tauri::http::header::CACHE_CONTROL, cache)
        .header(tauri::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap_or_else(|_| {
            tauri::http::Response::builder()
                .status(500)
                .body(Vec::new())
                .unwrap()
        })
}

fn error_response(status: u16) -> tauri::http::Response<Vec<u8>> {
    response(status, "text/plain", "no-store", Vec::new())
}

fn cache_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_cache_dir().ok()?.join("thumbs");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn thumb_cache_path(dir: &Path, source: &Path, edge: u32, mtime: u64, len: u64) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(b"photowall-thumb-v1");
    hasher.update(source.as_os_str().as_encoded_bytes());
    hasher.update(edge.to_le_bytes());
    hasher.update(mtime.to_le_bytes());
    hasher.update(len.to_le_bytes());
    let hex = format!("{:x}", hasher.finalize());
    dir.join(&hex[..2]).join(format!("{hex}.jpg"))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let tmp = path.with_extension("part");
    fs::write(&tmp, bytes).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        err.to_string()
    })
}

fn flatten_rgb(image: &image::DynamicImage) -> image::RgbImage {
    let rgba = image.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (pixel, out) in rgba.pixels().zip(rgb.pixels_mut()) {
        let alpha = u16::from(pixel[3]);
        let inverse = 255 - alpha;
        let background = 18u16;
        out[0] = ((u16::from(pixel[0]) * alpha + background * inverse) / 255) as u8;
        out[1] = ((u16::from(pixel[1]) * alpha + background * inverse) / 255) as u8;
        out[2] = ((u16::from(pixel[2]) * alpha + background * inverse) / 255) as u8;
    }
    rgb
}

fn render_jpeg_thumb(path: &Path, edge: u32) -> Result<Vec<u8>, String> {
    let reader = image::ImageReader::open(path).map_err(|err| err.to_string())?;
    let mut reader = reader
        .with_guessed_format()
        .map_err(|err| err.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoded = reader.decode().map_err(|err| err.to_string())?;
    if let Some(tag) = crate::scan::read_orientation(path) {
        if let Ok(tag) = u8::try_from(tag) {
            if let Some(orientation) = image::metadata::Orientation::from_exif(tag) {
                decoded.apply_orientation(orientation);
            }
        }
    }
    let (width, height) = fit_edge(decoded.width(), decoded.height(), edge);
    let resized = if width == decoded.width() && height == decoded.height() {
        decoded
    } else {
        let filter = if edge <= 96 {
            image::imageops::FilterType::Triangle
        } else {
            image::imageops::FilterType::CatmullRom
        };
        decoded.resize(width, height, filter)
    };
    let rgb = flatten_rgb(&resized);
    let mut bytes = Vec::new();
    let quality: u8 = if edge <= 96 { 55 } else { 78 };
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
    encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|err| err.to_string())?;
    Ok(bytes)
}

fn read_original(path: &Path) -> Result<(Vec<u8>, &'static str), String> {
    let meta = fs::metadata(path).map_err(|err| err.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > MAX_FULL_BYTES {
        return Err("file is larger than 120MB".into());
    }
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    Ok((bytes, mime_for(path)))
}

pub fn enqueue(app: AppHandle, raw_uri: String, responder: tauri::UriSchemeResponder) {
    std::thread::spawn(move || {
        let Some(parsed) = parse_media_uri(&raw_uri) else {
            responder.respond(error_response(400));
            return;
        };
        if !is_image(&parsed.path) || !parsed.path.is_file() {
            responder.respond(error_response(404));
            return;
        }
        match parsed.kind {
            Kind::Full => match read_original(&parsed.path) {
                Ok((bytes, mime)) => {
                    responder.respond(response(200, mime, "private, max-age=3600", bytes))
                }
                Err(_) => responder.respond(error_response(404)),
            },
            Kind::Thumb => {
                let edge = parsed.edge.clamp(16, 1600);
                let (mtime, len) = file_stamp(&parsed.path);
                let Some(dir) = cache_dir(&app) else {
                    responder.respond(error_response(500));
                    return;
                };
                let cache_path = thumb_cache_path(&dir, &parsed.path, edge, mtime, len);
                if let Ok(bytes) = fs::read(&cache_path) {
                    if !bytes.is_empty() {
                        responder.respond(response(
                            200,
                            "image/jpeg",
                            "public, max-age=31536000, immutable",
                            bytes,
                        ));
                        return;
                    }
                }
                let source = parsed.path.clone();
                pool().push(Box::new(move || {
                    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        render_jpeg_thumb(&source, edge)
                    }));
                    let response = match rendered {
                        Ok(Ok(bytes)) => {
                            let _ = write_atomic(&cache_path, &bytes);
                            response(
                                200,
                                "image/jpeg",
                                "public, max-age=31536000, immutable",
                                bytes,
                            )
                        }
                        _ => error_response(415),
                    };
                    responder.respond(response);
                }));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_edge_keeps_aspect_inside_the_box() {
        assert_eq!(fit_edge(400, 200, 100), (100, 50));
        assert_eq!(fit_edge(40, 20, 100), (40, 20));
        assert_eq!(fit_edge(0, 10, 32), (1, 10));
    }

    #[test]
    fn parses_thumb_and_full_urls() {
        let hello = URL_SAFE_NO_PAD.encode(b"hello");
        let thumb = parse_media_uri(&format!("thumb://localhost/thumb/64/10/20/{hello}")).unwrap();
        assert!(matches!(thumb.kind, Kind::Thumb));
        assert_eq!(thumb.edge, 64);
        assert_eq!(thumb.path, PathBuf::from("hello"));

        let full = parse_media_uri(&format!("http://thumb.localhost/full/0/1/2/{hello}")).unwrap();
        assert!(matches!(full.kind, Kind::Full));
        assert!(parse_media_uri("http://thumb.localhost/nope").is_none());
        assert!(
            parse_media_uri(&format!("http://thumb.localhost/thumb/64/1/2/{hello}extra")).is_none()
        );
    }

    #[test]
    fn sideways_jpeg_thumb_is_upright() {
        let dir = std::env::temp_dir().join(format!("photowall-orient-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let plain_path = dir.join("plain.jpg");
        let mut image = image::RgbImage::new(20, 10);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgb([30, 60, 90]);
        }
        image.save(&plain_path).unwrap();
        let plain = fs::read(&plain_path).unwrap();
        let sideways = dir.join("sideways.jpg");
        let mut oriented = vec![0xFF, 0xD8];
        oriented.extend_from_slice(&crate::scan::exif_orientation_segment(6));
        oriented.extend_from_slice(&plain[2..]);
        fs::write(&sideways, &oriented).unwrap();

        let bytes = render_jpeg_thumb(&sideways, 80).unwrap();
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert!(
            thumb.height() > thumb.width(),
            "expected a portrait thumb, got {}x{}",
            thumb.width(),
            thumb.height()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decodes_the_frontend_base64url_vectors() {
        for (bytes, encoded) in [
            (b"hello".as_slice(), "aGVsbG8"),
            (b"a/b c".as_slice(), "YS9iIGM"),
            (b"Photo Wall".as_slice(), "UGhvdG8gV2FsbA"),
            (
                "C:\\Photos\\图片.png".as_bytes(),
                "QzpcUGhvdG9zXOWbvueJhy5wbmc",
            ),
            (&[251, 239, 255], "--__"),
        ] {
            assert_eq!(URL_SAFE_NO_PAD.decode(encoded).unwrap(), bytes);
        }
    }
}
