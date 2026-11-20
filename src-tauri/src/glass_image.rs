//! Pictures on disk as glass backdrops.
//!
//! Responsibilities:
//! - List the stock wallpapers Windows ships under `%WINDIR%\Web`. They are
//!   read in place; nothing is copied or bundled.
//! - Decode a picked JPEG or PNG and average it into the same [`Backdrop`]
//!   shape [`crate::backdrop::sample`] produces, so the UI treats a picture
//!   exactly like the desktop wallpaper.
//! - Make small JPEG thumbnails for the Settings tiles, so the webview never
//!   loads a multi-megabyte 4K file.

use crate::backdrop::{self, Backdrop};
use base64::Engine;
use image::{DynamicImage, ImageReader, Limits, RgbImage};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::SystemTime;

/// SECURITY: a picked path is only ever read and decoded. Capping the file
/// size (and the decoded dimensions in [`decode`]) keeps a huge or hostile
/// file from exhausting memory.
pub const MAX_IMAGE_BYTES: u64 = 40 * 1024 * 1024;

const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];

/// Longest side of the copy kept for backdrops: well above the largest
/// sample grid (160 cells), small enough to keep in memory.
const WORKING_SIZE: u32 = 640;

/// Thumbnail size for the Settings tiles, 16:10 like most wallpapers.
const THUMB: (u32, u32) = (192, 120);

/// A wallpaper that ships with Windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockWallpaper {
    pub path: PathBuf,
    /// Folder and file name, e.g. `ThemeA img20`, for tooltips.
    pub name: String,
}

/// Whether `path` has an extension Pill decodes.
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Identifies one version of a file; a changed file gets a new key.
type Key = (PathBuf, Option<SystemTime>, u64);

/// Checks that `path` is a readable JPEG or PNG within [`MAX_IMAGE_BYTES`].
///
/// @param path File to check.
/// @returns A cache key for this version of the file, or a message for the user.
pub fn check_image_file(path: &Path) -> Result<(PathBuf, Option<SystemTime>, u64), String> {
    let meta = fs::metadata(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} isn't a file", path.display()));
    }
    if !is_image_path(path) {
        return Err("Pick a JPEG or PNG image".into());
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!("{} is larger than {} MB", path.display(), MAX_IMAGE_BYTES / (1024 * 1024)));
    }
    Ok((path.to_path_buf(), meta.modified().ok(), meta.len()))
}

// PERF: a full 4K decode holds ~30 MB; the tiles ask for a dozen at once, so
// only a few decode at a time.
static GATE: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());
const MAX_DECODES: usize = 3;

fn decode(path: &Path) -> Result<DynamicImage, String> {
    let (count, freed) = &GATE;
    {
        let mut n = freed.wait_while(count.lock().unwrap(), |n| *n >= MAX_DECODES).unwrap();
        *n += 1;
    }
    let result = (|| {
        let mut reader = ImageReader::open(path)
            .and_then(|r| r.with_guessed_format())
            .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        limits.max_alloc = Some(512 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().map_err(|e| format!("Couldn't open {} as an image: {e}", path.display()))
    })();
    *count.lock().unwrap() -= 1;
    freed.notify_one();
    result
}

/// The centered crop of a `w` × `h` image with the aspect ratio of
/// `target_w` × `target_h`, as `(x, y, width, height)`: how "fill" fits a
/// picture to a monitor.
fn cover(w: u32, h: u32, target_w: u32, target_h: u32) -> (u32, u32, u32, u32) {
    if w == 0 || h == 0 || target_w == 0 || target_h == 0 {
        return (0, 0, w, h);
    }
    // Compare w/h against target_w/target_h without floating point.
    if u64::from(w) * u64::from(target_h) > u64::from(h) * u64::from(target_w) {
        let cw = ((u64::from(h) * u64::from(target_w)) / u64::from(target_h)).max(1) as u32;
        ((w - cw) / 2, 0, cw, h)
    } else {
        let ch = ((u64::from(w) * u64::from(target_h)) / u64::from(target_w)).max(1) as u32;
        (0, (h - ch) / 2, w, ch)
    }
}

static WORKING: Mutex<Option<(Key, Arc<RgbImage>)>> = Mutex::new(None);

/// The picked image shrunk to [`WORKING_SIZE`], decoded once per file
/// version. The backdrop watch asks every few seconds, and both windows ask
/// on every move to another monitor.
fn working_copy(path: &Path) -> Result<Arc<RgbImage>, String> {
    let key = check_image_file(path)?;
    if let Some((k, img)) = WORKING.lock().unwrap().as_ref() {
        if *k == key {
            return Ok(img.clone());
        }
    }
    let img = Arc::new(decode(path)?.thumbnail(WORKING_SIZE, WORKING_SIZE).to_rgb8());
    *WORKING.lock().unwrap() = Some((key, img.clone()));
    Ok(img)
}

/// The picture at `path` filling `monitor`, as a [`Backdrop`].
///
/// @param path JPEG or PNG on disk.
/// @param monitor `(x, y, width, height)` in physical screen pixels.
/// @returns The backdrop, or `None` (logged) if the file can't be used.
pub fn backdrop(path: &Path, monitor: (i32, i32, u32, u32)) -> Option<Backdrop> {
    let (_, _, mw, mh) = monitor;
    if mw == 0 || mh == 0 {
        return None;
    }
    let img = working_copy(path).map_err(|e| log::warn!("glass image: {e}")).ok()?;
    let (x, y, w, h) = cover(img.width(), img.height(), mw, mh);
    let (out_w, out_h) = backdrop::grid(mw as usize, mh as usize);
    let region = (x as usize, y as usize, w as usize, h as usize);
    Some(Backdrop {
        width: out_w as u32,
        height: out_h as u32,
        pixels: backdrop::downsample(img.as_raw(), backdrop::RGB, img.width() as usize, region, out_w, out_h),
        monitor,
        scale: 1.0,
        image: Some(path.to_string_lossy().into_owned()),
    })
}

static THUMBS: Mutex<Option<HashMap<Key, String>>> = Mutex::new(None);

/// A small JPEG of `path`, cropped to 16:10, as a `data:` URL.
///
/// @param path JPEG or PNG on disk.
/// @returns The data URL, or a message for the user.
pub fn thumbnail(path: &Path) -> Result<String, String> {
    let key = check_image_file(path)?;
    if let Some(url) = THUMBS.lock().unwrap().as_ref().and_then(|m| m.get(&key)) {
        return Ok(url.clone());
    }
    let img = decode(path)?;
    let (x, y, w, h) = cover(img.width(), img.height(), THUMB.0, THUMB.1);
    let small = img.crop_imm(x, y, w, h).thumbnail_exact(THUMB.0, THUMB.1).to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 82)
        .encode_image(&small)
        .map_err(|e| e.to_string())?;
    let url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&jpeg));
    let mut thumbs = THUMBS.lock().unwrap();
    let map = thumbs.get_or_insert_with(HashMap::new);
    // Old versions of edited files would otherwise pile up.
    map.retain(|(p, ..), _| p != &key.0);
    map.insert(key, url.clone());
    Ok(url)
}

/// The wallpapers Windows ships, read from `%WINDIR%\Web\Wallpaper` and
/// `%WINDIR%\Web\4K\Wallpaper`.
pub fn stock_wallpapers() -> Vec<StockWallpaper> {
    let Some(windir) = std::env::var_os("WINDIR") else { return Vec::new() };
    let web = PathBuf::from(windir).join("Web");
    list_stock(&[web.join("Wallpaper"), web.join("4K").join("Wallpaper")])
}

/// Images directly in each root and one folder down, sorted, with
/// duplicates dropped.
///
/// NOTE: `4K\Wallpaper\Windows\img0_1920x1200.jpg` is `Wallpaper\Windows\img0.jpg`
/// at another resolution, so a file counts as already listed when its
/// folder and the stem before `_` match. Earlier roots win.
fn list_stock(roots: &[PathBuf]) -> Vec<StockWallpaper> {
    fn sorted(dir: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        entries.sort();
        entries
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in roots {
        let mut dirs = vec![root.clone()];
        dirs.extend(sorted(root).into_iter().filter(|p| p.is_dir()));
        for dir in dirs {
            let folder = dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            for file in sorted(&dir) {
                if !is_image_path(&file) || !file.is_file() {
                    continue;
                }
                let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let base = stem.split('_').next().unwrap_or(&stem).to_ascii_lowercase();
                if seen.insert((folder.to_ascii_lowercase(), base)) {
                    out.push(StockWallpaper { name: format!("{folder} {stem}"), path: file });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_jpeg_and_png_count_as_images() {
        assert!(is_image_path(Path::new("a/img0.jpg")));
        assert!(is_image_path(Path::new("a/IMG0.JPEG")));
        assert!(is_image_path(Path::new("a/b.Png")));
        assert!(!is_image_path(Path::new("a/Desktop.ini")));
        assert!(!is_image_path(Path::new("a/img.webp")));
        assert!(!is_image_path(Path::new("a/jpg")));
    }

    #[test]
    fn cover_crops_to_the_target_aspect() {
        assert_eq!(cover(400, 100, 200, 100), (100, 0, 200, 100));
        assert_eq!(cover(100, 400, 100, 200), (0, 100, 100, 200));
        assert_eq!(cover(160, 100, 16, 10), (0, 0, 160, 100));
    }

    #[test]
    fn stock_listing_filters_and_drops_resolution_duplicates() {
        let dir = std::env::temp_dir().join(format!("pill-stock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let web = dir.join("Wallpaper");
        let k4 = dir.join("4K").join("Wallpaper");
        for (folder, files) in [
            (web.join("Windows"), &["img0.jpg", "img19.jpg"][..]),
            (web.join("ThemeA"), &["Desktop.ini", "img20.jpg", "notes.txt"][..]),
            (k4.join("Windows"), &["img0_1920x1200.jpg", "img0_3840x2160.jpg", "img7_1024x768.png"][..]),
        ] {
            fs::create_dir_all(&folder).unwrap();
            for f in files {
                fs::write(folder.join(f), b"x").unwrap();
            }
        }
        fs::create_dir_all(web.join("Empty.jpg")).unwrap();

        let names: Vec<String> = list_stock(&[web, k4, dir.join("missing")]).into_iter().map(|w| w.name).collect();
        assert_eq!(names, ["ThemeA img20", "Windows img0", "Windows img19", "Windows img7_1024x768"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_folders_and_other_types() {
        let dir = std::env::temp_dir().join(format!("pill-glass-check-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), b"x").unwrap();
        fs::write(dir.join("a.png"), b"x").unwrap();
        assert!(check_image_file(&dir).is_err());
        assert!(check_image_file(&dir.join("a.txt")).is_err());
        assert!(check_image_file(&dir.join("missing.png")).is_err());
        assert!(check_image_file(&dir.join("a.png")).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn decodes_a_png_into_a_backdrop() {
        let dir = std::env::temp_dir().join(format!("pill-glass-png-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("red.png");
        RgbImage::from_pixel(64, 40, image::Rgb([200, 10, 10])).save(&path).unwrap();
        let b = backdrop(&path, (0, 0, 1920, 1200)).unwrap();
        assert_eq!((b.width, b.height), (61, 38));
        // Resampling may round a level off.
        for (got, want) in b.pixels[..3].iter().zip([200u8, 10, 10]) {
            assert!(got.abs_diff(want) <= 2, "{got} vs {want}");
        }
        assert_eq!(b.image.as_deref(), Some(path.to_string_lossy().as_ref()));
        assert!(thumbnail(&path).unwrap().starts_with("data:image/jpeg;base64,"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
