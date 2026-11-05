use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

static SCAN_GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRecord {
    pub path: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: String,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    phase: String,
    done: usize,
    total: usize,
    generation: u64,
}

pub fn is_image(path: &Path) -> bool {
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif"];
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| EXTS.iter().any(|allowed| ext.eq_ignore_ascii_case(allowed)))
        .unwrap_or(false)
}

pub fn is_hidden(path: &Path) -> bool {
    let name_hidden = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false);
    if name_hidden {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if let Ok(meta) = fs::metadata(path) {
            if meta.file_attributes() & 0x2 != 0 {
                return true;
            }
        }
    }
    false
}

pub fn file_stamp(path: &Path) -> (u64, u64) {
    let Ok(meta) = fs::metadata(path) else {
        return (0, 0);
    };
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    (modified_ms, meta.len())
}

pub fn apply_orientation(width: u32, height: u32, orientation: Option<u32>) -> (u32, u32) {
    match orientation {
        Some(5) | Some(6) | Some(7) | Some(8) => (height, width),
        _ => (width, height),
    }
}

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left), Some(right)) if left.is_ascii_digit() && right.is_ascii_digit() => {
                let mut left_num = 0u128;
                let mut right_num = 0u128;
                while a_chars
                    .peek()
                    .map(|ch| ch.is_ascii_digit())
                    .unwrap_or(false)
                {
                    left_num = left_num
                        .saturating_mul(10)
                        .saturating_add(u128::from(a_chars.next().unwrap().to_digit(10).unwrap()));
                }
                while b_chars
                    .peek()
                    .map(|ch| ch.is_ascii_digit())
                    .unwrap_or(false)
                {
                    right_num = right_num
                        .saturating_mul(10)
                        .saturating_add(u128::from(b_chars.next().unwrap().to_digit(10).unwrap()));
                }
                match left_num.cmp(&right_num) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            _ => {
                let left = a_chars.next().unwrap().to_ascii_lowercase();
                let right = b_chars.next().unwrap().to_ascii_lowercase();
                match left.cmp(&right) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
        }
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn relative_name(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        if let std::path::Component::Normal(part) = component {
            parts.push(part.to_string_lossy().into_owned());
        }
    }
    if parts.is_empty() {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    } else {
        parts.join("/")
    }
}

fn read_prefix(path: &Path, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; limit];
    let read = file.read(&mut buf).ok()?;
    buf.truncate(read);
    Some(buf)
}

fn read_u16(bytes: &[u8], offset: usize, little: bool) -> Option<u16> {
    let chunk = bytes.get(offset..offset + 2)?;
    Some(if little {
        u16::from_le_bytes([chunk[0], chunk[1]])
    } else {
        u16::from_be_bytes([chunk[0], chunk[1]])
    })
}

fn read_u32(bytes: &[u8], offset: usize, little: bool) -> Option<u32> {
    let chunk = bytes.get(offset..offset + 4)?;
    Some(if little {
        u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
    } else {
        u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
    })
}

fn tiff_orientation(tiff: &[u8]) -> Option<u32> {
    if tiff.len() < 8 {
        return None;
    }
    let little = match &tiff[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if read_u16(tiff, 2, little)? != 42 {
        return None;
    }
    let mut cursor = read_u32(tiff, 4, little)? as usize;
    if cursor + 2 > tiff.len() {
        return None;
    }
    let count = read_u16(tiff, cursor, little)? as usize;
    cursor += 2;
    for _ in 0..count {
        if cursor + 12 > tiff.len() {
            return None;
        }
        let tag = read_u16(tiff, cursor, little)?;
        let kind = read_u16(tiff, cursor + 2, little)?;
        let values = read_u32(tiff, cursor + 4, little)?;
        if tag == 0x0112 && kind == 3 && values >= 1 {
            return Some(u32::from(read_u16(tiff, cursor + 8, little)?));
        }
        cursor += 12;
    }
    None
}

fn exif_app1_orientation(segment: &[u8]) -> Option<u32> {
    let prefix = b"Exif\0\0";
    if segment.len() < prefix.len() + 8 || &segment[..prefix.len()] != prefix {
        return None;
    }
    tiff_orientation(&segment[prefix.len()..])
}

/// Reads the EXIF orientation tag from a JPEG without decoding pixels.
pub(crate) fn read_orientation(path: &Path) -> Option<u32> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext != "jpg" && ext != "jpeg" {
        return None;
    }
    let bytes = read_prefix(path, 256 * 1024)?;
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut index = 2usize;
    while index + 4 <= bytes.len() {
        if bytes[index] != 0xFF {
            return None;
        }
        index += 1;
        while index < bytes.len() && bytes[index] == 0xFF {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        let marker = bytes[index];
        index += 1;
        if marker == 0x01 || marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if index + 2 > bytes.len() {
            return None;
        }
        let len = usize::from(u16::from_be_bytes([bytes[index], bytes[index + 1]]));
        if len < 2 || index + len > bytes.len() {
            return None;
        }
        if marker == 0xE1 {
            if let Some(orientation) = exif_app1_orientation(&bytes[index + 2..index + len]) {
                return Some(orientation);
            }
        }
        if marker == 0xDA {
            break;
        }
        index += len;
    }
    None
}

#[cfg(test)]
pub(crate) fn exif_orientation_segment(orientation: u16) -> Vec<u8> {
    let mut tiff = vec![
        b'I', b'I', 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x12, 0x01, 0x03, 0x00, 0x01,
        0x00, 0x00, 0x00,
    ];
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let mut segment = b"Exif\0\0".to_vec();
    segment.extend_from_slice(&tiff);
    let mut marker = vec![0xFF, 0xE1];
    marker.extend_from_slice(&((segment.len() + 2) as u16).to_be_bytes());
    marker.extend_from_slice(&segment);
    marker
}

fn dimensions(path: &Path) -> (u32, u32) {
    let Ok(size) = imagesize::size(path) else {
        return (4, 3);
    };
    let width = u32::try_from(size.width).unwrap_or(0);
    let height = u32::try_from(size.height).unwrap_or(0);
    if width == 0 || height == 0 {
        return (4, 3);
    }
    apply_orientation(width, height, read_orientation(path))
}

fn measure(root: &Path, path: &Path) -> ImageRecord {
    let (modified_ms, bytes) = file_stamp(path);
    let (width, height) = dimensions(path);
    ImageRecord {
        name: relative_name(root, path),
        path: path.to_string_lossy().into_owned(),
        width,
        height,
        bytes,
        modified_ms,
    }
}

fn list_files(
    root: &Path,
    recursive: bool,
    cancel: &AtomicBool,
    progress: &impl Fn(ScanProgress),
) -> Result<Vec<PathBuf>, String> {
    if !root.is_dir() {
        return Err(format!("Not a folder: {}", root.display()));
    }
    let mut files = Vec::new();
    if !recursive {
        for entry in fs::read_dir(root).map_err(|err| err.to_string())? {
            if cancel.load(AtomicOrdering::Relaxed) {
                return Err("cancelled".into());
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.is_file() && !is_hidden(&path) && is_image(&path) {
                files.push(path);
            }
        }
        progress(ScanProgress {
            phase: "listing".into(),
            done: files.len(),
            total: files.len(),
        });
        return Ok(files);
    }

    let walker = WalkDir::new(root).follow_links(false).into_iter();
    let filtered = walker.filter_entry(|entry| entry.depth() == 0 || !is_hidden(entry.path()));
    for entry in filtered {
        if cancel.load(AtomicOrdering::Relaxed) {
            return Err("cancelled".into());
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.depth() == 0 || !entry.file_type().is_file() {
            continue;
        }
        let path = entry.into_path();
        if is_hidden(&path) || !is_image(&path) {
            continue;
        }
        files.push(path);
        if files.len() % 400 == 0 {
            progress(ScanProgress {
                phase: "listing".into(),
                done: files.len(),
                total: 0,
            });
        }
    }
    progress(ScanProgress {
        phase: "listing".into(),
        done: files.len(),
        total: files.len(),
    });
    Ok(files)
}

fn measure_all(
    root: &Path,
    paths: &[PathBuf],
    cancel: &AtomicBool,
    progress: &(impl Fn(ScanProgress) + Sync),
) -> Result<Vec<ImageRecord>, String> {
    let total = paths.len();
    if total == 0 {
        return Ok(Vec::new());
    }
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(2, 8)
        .min(total);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let slots = Mutex::new(vec![None; total]);

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                if cancel.load(AtomicOrdering::Relaxed) {
                    return;
                }
                let index = next.fetch_add(1, AtomicOrdering::Relaxed);
                if index >= total {
                    return;
                }
                let record = measure(root, &paths[index]);
                slots.lock().unwrap()[index] = Some(record);
                let finished = done.fetch_add(1, AtomicOrdering::Relaxed) + 1;
                if finished == total || finished % 40 == 0 {
                    progress(ScanProgress {
                        phase: "measuring".into(),
                        done: finished,
                        total,
                    });
                }
            });
        }
    });

    if cancel.load(AtomicOrdering::Relaxed) {
        return Err("cancelled".into());
    }
    let mut records = Vec::with_capacity(total);
    for slot in slots.into_inner().unwrap() {
        records.push(slot.ok_or_else(|| "incomplete scan".to_string())?);
    }
    records.sort_by(|a, b| natural_cmp(&a.name, &b.name).then_with(|| a.path.cmp(&b.path)));
    Ok(records)
}

pub fn collect_images(
    root: &Path,
    recursive: bool,
    cancel: &AtomicBool,
    progress: &(impl Fn(ScanProgress) + Sync),
) -> Result<Vec<ImageRecord>, String> {
    let root = absolute_path(root);
    let files = list_files(&root, recursive, cancel, progress)?;
    measure_all(&root, &files, cancel, progress)
}

#[tauri::command]
pub async fn scan_folder(
    app: AppHandle,
    path: String,
    recursive: bool,
) -> Result<Vec<ImageRecord>, String> {
    let generation = SCAN_GEN.fetch_add(1, AtomicOrdering::Relaxed) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let cancel = AtomicBool::new(false);
        let progress = |update: ScanProgress| {
            if SCAN_GEN.load(AtomicOrdering::Relaxed) != generation {
                cancel.store(true, AtomicOrdering::Relaxed);
                return;
            }
            let _ = app.emit(
                "scan-progress",
                &ProgressEvent {
                    phase: update.phase,
                    done: update.done,
                    total: update.total,
                    generation,
                },
            );
        };
        if SCAN_GEN.load(AtomicOrdering::Relaxed) != generation {
            return Err("cancelled".into());
        }
        collect_images(Path::new(&path), recursive, &cancel, &progress)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn normalize_drop_path(path: String) -> Result<String, String> {
    let path = absolute_path(Path::new(&path));
    if path.is_dir() {
        return Ok(path.to_string_lossy().into_owned());
    }
    if path.is_file() {
        if let Some(parent) = path.parent() {
            if parent.as_os_str().is_empty() {
                return Err("Drop a folder of photos".into());
            }
            return Ok(parent.to_string_lossy().into_owned());
        }
    }
    Err("Drop a folder of photos".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn write_rgb(path: &Path, width: u32, height: u32) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut image = ImageBuffer::<Rgb<u8>, _>::new(width, height);
        for pixel in image.pixels_mut() {
            *pixel = Rgb([40, 90, 140]);
        }
        image.save(path).unwrap();
    }

    #[test]
    fn natural_order_puts_2_before_10() {
        let mut names = vec!["b10.jpg", "a.jpg", "b2.jpg", "B1.jpg"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["a.jpg", "B1.jpg", "b2.jpg", "b10.jpg"]);
    }

    #[test]
    fn orientation_swaps_only_the_sideways_values() {
        assert_eq!(apply_orientation(30, 10, Some(6)), (10, 30));
        assert_eq!(apply_orientation(30, 10, Some(1)), (30, 10));
        assert_eq!(apply_orientation(30, 10, None), (30, 10));
    }

    #[test]
    fn scan_skips_hidden_files_and_non_images() {
        let dir = std::env::temp_dir().join(format!("photowall-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        write_rgb(&dir.join("a.jpg"), 20, 10);
        write_rgb(&dir.join(".hidden.jpg"), 8, 8);
        write_rgb(&dir.join("sub").join("b.png"), 10, 30);
        write_rgb(&dir.join(".secret").join("nested.png"), 12, 12);
        fs::write(dir.join("notes.txt"), b"nope").unwrap();

        let cancel = AtomicBool::new(false);
        let top = collect_images(&dir, false, &cancel, &|_| {}).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].width, 20);
        assert_eq!(top[0].height, 10);
        assert!(top[0].name.ends_with("a.jpg"));

        let sideways = dir.join("sideways.jpg");
        let plain = fs::read(&dir.join("a.jpg")).unwrap();
        assert_eq!(&plain[..2], &[0xFF, 0xD8]);
        let mut oriented = vec![0xFF, 0xD8];
        oriented.extend_from_slice(&exif_orientation_segment(6));
        oriented.extend_from_slice(&plain[2..]);
        fs::write(&sideways, &oriented).unwrap();
        assert_eq!(read_orientation(&sideways), Some(6));
        assert_eq!(dimensions(&sideways), (10, 20));

        let all = collect_images(&dir, true, &cancel, &|_| {}).unwrap();
        let names: Vec<_> = all.iter().map(|record| record.name.as_str()).collect();
        assert!(names.iter().any(|name| name.ends_with("a.jpg")));
        assert!(names.iter().any(|name| name.contains("b.png")));
        assert!(!names.iter().any(|name| name.contains("hidden")));
        assert!(!names.iter().any(|name| name.contains("nested")));
        assert!(!names.iter().any(|name| name.contains("secret")));
        assert!(!names.iter().any(|name| name.ends_with(".txt")));
        let _ = fs::remove_dir_all(&dir);
    }
}
