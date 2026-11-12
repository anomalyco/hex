//! A capture bundle's light copy as one zip file (Labs › Copy captures as a
//! zip): everything a reader needs to understand the capture — the manifest,
//! the summary, the contact sheet, and its keyframes — without the video,
//! audio, or full-size frames.
//!
//! The copy is STORED (no compression: frames are already compressed PNGs),
//! so it needs no archive dependency: local headers, a central directory, and
//! an end-of-central-directory record, all written by hand. It sits beside
//! the bundle's other files, `<folder name>.slim.zip`, so capture retention
//! removes it with the bundle. Its `summary.md` is generated with the bundle
//! folder as `.`, so the copy reads the same wherever it is unzipped.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};

use crate::handoff::{
    HEAVY_MEDIA, MEDIA_PENDING, MEDIA_SENDING, SLIM_ZIP_SUFFIX, SLIM_ZIP_TEMP_SUFFIX,
};

/// The summary the copy rewrites for its new location.
const SUMMARY: &str = "summary.md";

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL: u32 = 0x0605_4b50;
/// Bit 11: names are UTF-8.
const UTF8_FLAG: u16 = 1 << 11;
const STORED: u16 = 0;
const VERSION: u16 = 20;
/// `version made by`: Unix (3), so external attributes are read as a mode.
const MADE_BY: u16 = (3 << 8) | VERSION;
/// `-rw-r--r--`.
const FILE_MODE: u32 = 0o100_644;

/// `<folder name>.slim.zip` inside `bundle_dir`.
pub fn path_in(bundle_dir: &Path) -> Result<PathBuf> {
    let name = bundle_dir
        .file_name()
        .and_then(|name| name.to_str())
        .context("Bundle folder has no name")?;
    Ok(bundle_dir.join(format!("{name}{SLIM_ZIP_SUFFIX}")))
}

/// Writes `bundle_dir`'s light copy inside it, atomically, and returns its
/// path. `summary.md` in the copy names the bundle folder as `.`; the copy on
/// disk is never touched.
pub fn write(bundle_dir: &Path) -> Result<PathBuf> {
    let (directory, bundle) = crate::handoff::load_bundle(bundle_dir)?;
    let destination = path_in(&directory)?;
    let temp = directory.join(format!(
        "{}.tmp",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .context("Light copy has no name")?
    ));
    let entries = entries(&directory, &bundle)?;
    let written = write_zip(&temp, &entries)
        .and_then(|()| fs::rename(&temp, &destination).map_err(Into::into));
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(error).with_context(|| format!("Could not write {}", destination.display()));
    }
    Ok(destination)
}

/// One file the copy carries.
struct Entry {
    /// Its path inside the zip, with forward slashes.
    name: String,
    path: PathBuf,
    size: u64,
    modified: SystemTime,
    /// The bytes to store instead of the file's, for the rewritten summary.
    data: Option<Vec<u8>>,
}

impl Entry {
    fn crc(&self) -> Result<u32> {
        match &self.data {
            Some(data) => Ok(crc32(data)),
            None => crc32_file(&self.path),
        }
    }

    fn open(&self) -> Result<Box<dyn Read + '_>> {
        match &self.data {
            Some(data) => Ok(Box::new(data.as_slice())),
            None => Ok(Box::new(File::open(&self.path).with_context(|| {
                format!("Could not read {}", self.path.display())
            })?)),
        }
    }
}

/// Every light file of the bundle, sorted by name, with the summary rewritten
/// for a copy that is unzipped somewhere else.
fn entries(directory: &Path, bundle: &crate::model::ScrawlBundle) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    collect(directory, Path::new(""), &mut entries)?;
    // The header names the bundle folder; `.` reads right wherever the copy is
    // unzipped, and nothing else in the summary names it.
    let summary = crate::sketch::summary_markdown(bundle, Path::new("."));
    let modified = fs::metadata(directory.join(SUMMARY))
        .and_then(|meta| meta.modified())
        .unwrap_or_else(|_| current_time());
    let summary = summary.into_bytes();
    match entries.iter_mut().find(|entry| entry.name == SUMMARY) {
        Some(entry) => {
            entry.size = summary.len() as u64;
            entry.data = Some(summary);
        }
        None => entries.push(Entry {
            name: SUMMARY.into(),
            path: directory.join(SUMMARY),
            size: summary.len() as u64,
            modified,
            data: Some(summary),
        }),
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn collect(root: &Path, relative: &Path, out: &mut Vec<Entry>) -> Result<()> {
    for item in fs::read_dir(root).with_context(|| format!("Could not read {}", root.display()))? {
        let item = item?;
        let Some(name) = item.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if excluded(&name) {
            continue;
        }
        let file_type = item.file_type()?;
        // A symlink could point anywhere, including outside the bundle.
        if file_type.is_symlink() {
            continue;
        }
        let relative = relative.join(&name);
        if file_type.is_dir() {
            collect(&item.path(), &relative, out)?;
        } else if file_type.is_file() {
            let metadata = item.metadata()?;
            out.push(Entry {
                name: zip_name(&relative)?,
                path: item.path(),
                size: metadata.len(),
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
                data: None,
            });
        }
    }
    Ok(())
}

/// The files and markers a light copy leaves out, at any depth: the heavy
/// media a bundle copies separately, this machine's delivery markers, and any
/// earlier copy.
fn excluded(name: &str) -> bool {
    HEAVY_MEDIA.contains(&name)
        || name == MEDIA_PENDING
        || name == MEDIA_SENDING
        || name == scrawl_router::delivery::PENDING
        || name == format!("{}.tmp", scrawl_router::delivery::PENDING)
        || name.ends_with(SLIM_ZIP_SUFFIX)
        || name.ends_with(SLIM_ZIP_TEMP_SUFFIX)
}

/// `path` with forward slashes and no prefix or `..`: what a zip names.
fn zip_name(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => {
                parts.push(name.to_str().context("File name is not UTF-8")?);
            }
            _ => bail!("Invalid path in bundle: {}", path.display()),
        }
    }
    Ok(parts.join("/"))
}

/// A bundle with no `summary.md` on disk has no file time; its entry is being
/// written now.
#[allow(clippy::disallowed_methods)] // no injected clock reaches this crate
fn current_time() -> SystemTime {
    SystemTime::now()
}

fn write_zip(path: &Path, entries: &[Entry]) -> Result<()> {
    ensure!(
        entries.len() < usize::from(u16::MAX),
        "Too many files for one capture copy"
    );
    let total: u64 = entries.iter().map(|entry| entry.size).sum();
    ensure!(
        total < u64::from(u32::MAX),
        "The capture's light files are too large for one zip"
    );
    let mut file =
        File::create(path).with_context(|| format!("Could not write {}", path.display()))?;
    let mut offset = 0_u64;
    let mut central = Vec::with_capacity(entries.len() * 64);
    for entry in entries {
        let name = entry.name.as_bytes();
        ensure!(
            name.len() < usize::from(u16::MAX),
            "A file name is too long for one zip"
        );
        let size = u32::try_from(entry.size).context("A light file is too large for one zip")?;
        let crc = entry.crc()?;
        let (time, date) = dos_time(entry.modified);
        let mut header = Vec::with_capacity(30 + name.len());
        put_u32(&mut header, LOCAL_HEADER);
        put_u16(&mut header, VERSION);
        put_u16(&mut header, UTF8_FLAG);
        put_u16(&mut header, STORED);
        put_u16(&mut header, time);
        put_u16(&mut header, date);
        put_u32(&mut header, crc);
        put_u32(&mut header, size);
        put_u32(&mut header, size);
        put_u16(&mut header, name.len() as u16);
        put_u16(&mut header, 0);
        header.extend_from_slice(name);
        let at =
            u32::try_from(offset).context("The capture's light files are too large for one zip")?;
        file.write_all(&header)?;
        let mut source = entry.open()?;
        let copied = std::io::copy(&mut source, &mut file)?;
        ensure!(
            copied == u64::from(size),
            "{} changed while it was copied",
            entry.path.display()
        );
        central.extend_from_slice(&central_header(name, crc, size, time, date, at));
        offset += header.len() as u64 + u64::from(size);
    }
    let central_at =
        u32::try_from(offset).context("The capture's light files are too large for one zip")?;
    let central_size = u32::try_from(central.len())
        .context("The capture's light files are too large for one zip")?;
    let archived = offset + central.len() as u64;
    ensure!(
        archived < u64::from(u32::MAX),
        "The capture's light files are too large for one zip"
    );
    file.write_all(&central)?;
    let mut end = Vec::with_capacity(22);
    put_u32(&mut end, END_OF_CENTRAL);
    put_u16(&mut end, 0);
    put_u16(&mut end, 0);
    put_u16(&mut end, entries.len() as u16);
    put_u16(&mut end, entries.len() as u16);
    put_u32(&mut end, central_size);
    put_u32(&mut end, central_at);
    put_u16(&mut end, 0);
    file.write_all(&end)?;
    file.sync_all()?;
    Ok(())
}

fn central_header(name: &[u8], crc: u32, size: u32, time: u16, date: u16, at: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(46 + name.len());
    put_u32(&mut header, CENTRAL_HEADER);
    put_u16(&mut header, MADE_BY);
    put_u16(&mut header, VERSION);
    put_u16(&mut header, UTF8_FLAG);
    put_u16(&mut header, STORED);
    put_u16(&mut header, time);
    put_u16(&mut header, date);
    put_u32(&mut header, crc);
    put_u32(&mut header, size);
    put_u32(&mut header, size);
    put_u16(&mut header, name.len() as u16);
    put_u16(&mut header, 0);
    put_u16(&mut header, 0);
    put_u16(&mut header, 0);
    put_u16(&mut header, 0);
    put_u32(&mut header, FILE_MODE << 16);
    put_u32(&mut header, at);
    header.extend_from_slice(name);
    header
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// The zip's DOS date and time for `time`, in UTC: seconds are stored in two.
/// A zip stores local time, but this crate has no local-time facility without
/// a new dependency, so a reader in another time zone may show a shifted time.
fn dos_time(time: SystemTime) -> (u16, u16) {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (mut year, month, day) = civil_from_days(days);
    let hour = (rest / 3_600) as u16;
    let minute = ((rest % 3_600) / 60) as u16;
    let second = (rest % 60) as u16;
    year = year.clamp(1980, 2107);
    let date = (((year - 1980) as u16) << 9) | ((month as u16) << 5) | day as u16;
    let clock = (hour << 11) | (minute << 5) | (second / 2);
    (clock, date)
}

/// The civil date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = (days - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_of_year = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_of_year + 2) / 5 + 1) as u32;
    let month = if month_of_year < 10 {
        month_of_year + 3
    } else {
        month_of_year - 9
    } as u32;
    (year + i64::from(month <= 2), month, day)
}

/// CRC-32 (IEEE 802.3), the checksum a zip stores.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

fn crc32_file(path: &Path) -> Result<u32> {
    let mut file =
        File::open(path).with_context(|| format!("Could not read {}", path.display()))?;
    let mut crc = !0_u32;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for &byte in &buffer[..read] {
            crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8);
        }
    }
    Ok(!crc)
}

/// One byte's polynomial, computed at compile time.
const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                0xedb8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "scrawl-slim-zip-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn bundle(&self) -> PathBuf {
            let path = self.0.join("1234.scrawl");
            fs::create_dir_all(path.join("frames")).unwrap();
            fs::create_dir_all(path.join("frames-full")).unwrap();
            let keyframes: Vec<_> = [0, 1_000]
                .into_iter()
                .enumerate()
                .map(|(index, t)| {
                    let file = format!("frames/frame-{index:03}.png");
                    fs::write(path.join(&file), b"fixture png").unwrap();
                    serde_json::json!({"index": index, "t": t, "reason": "initial", "file": file, "event": ""})
                })
                .collect();
            let bundle = serde_json::json!({
                "version": 1, "id": "1234", "createdAtMs": 1, "durationMs": 2_000,
                "target": {"mode": "display", "displayId": 1,
                    "frame": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0, "scale": 1.0}},
                "transcript": {"source": "none", "text": "", "segments": []},
                "strokes": [], "marks": [], "events": [], "keyframes": keyframes,
                "contactSheet": "contact-sheet.png"
            });
            fs::write(
                path.join("scrawl.json"),
                serde_json::to_vec(&bundle).unwrap(),
            )
            .unwrap();
            fs::write(path.join("summary.md"), "stale").unwrap();
            fs::write(path.join("contact-sheet.png"), b"sheet").unwrap();
            fs::write(path.join("audio.wav"), b"heavy").unwrap();
            fs::write(path.join("recording.mp4"), b"heavy").unwrap();
            fs::write(path.join("contact-sheet-full.png"), b"heavy").unwrap();
            fs::write(path.join("frames-full/frame-000.png"), b"heavy").unwrap();
            fs::write(path.join(".media-pending"), b"").unwrap();
            fs::write(path.join(".media-sending"), b"").unwrap();
            fs::write(path.join(scrawl_router::delivery::PENDING), b"{}").unwrap();
            fs::write(
                path.join(format!("{}.tmp", scrawl_router::delivery::PENDING)),
                b"{}",
            )
            .unwrap();
            fs::write(path.join("1234.scrawl.slim.zip"), b"older copy").unwrap();
            fs::write(path.join("1234.scrawl.slim.zip.tmp"), b"partial copy").unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// One entry read back from a zip's central directory.
    struct Read {
        name: String,
        data: Vec<u8>,
    }

    fn read_zip(path: &Path) -> Vec<Read> {
        let bytes = fs::read(path).unwrap();
        let end = bytes.len() - 22;
        assert_eq!(
            u32::from_le_bytes(bytes[end..end + 4].try_into().unwrap()),
            END_OF_CENTRAL
        );
        let count = u16::from_le_bytes(bytes[end + 10..end + 12].try_into().unwrap()) as usize;
        let central_size =
            u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap()) as usize;
        let central_at = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
        assert_eq!(central_at + central_size, end);
        let mut at = central_at;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            assert_eq!(
                u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()),
                CENTRAL_HEADER
            );
            let crc = u32::from_le_bytes(bytes[at + 16..at + 20].try_into().unwrap());
            let size = u32::from_le_bytes(bytes[at + 20..at + 24].try_into().unwrap()) as usize;
            let name_len = u16::from_le_bytes(bytes[at + 28..at + 30].try_into().unwrap()) as usize;
            let extra_len =
                u16::from_le_bytes(bytes[at + 30..at + 32].try_into().unwrap()) as usize;
            let comment_len =
                u16::from_le_bytes(bytes[at + 32..at + 34].try_into().unwrap()) as usize;
            let local_at = u32::from_le_bytes(bytes[at + 42..at + 46].try_into().unwrap()) as usize;
            let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
            assert_eq!(
                u32::from_le_bytes(bytes[local_at..local_at + 4].try_into().unwrap()),
                LOCAL_HEADER
            );
            let local_name =
                u16::from_le_bytes(bytes[local_at + 26..local_at + 28].try_into().unwrap())
                    as usize;
            let local_extra =
                u16::from_le_bytes(bytes[local_at + 28..local_at + 30].try_into().unwrap())
                    as usize;
            let start = local_at + 30 + local_name + local_extra;
            let data = bytes[start..start + size].to_vec();
            assert_eq!(crc32(&data), crc, "{name}");
            entries.push(Read { name, data });
            at += 46 + name_len + extra_len + comment_len;
        }
        entries
    }

    #[test]
    fn crc32_matches_the_standard_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn a_light_copy_carries_the_summary_and_leaves_the_media_out() {
        let fixture = Fixture::new();
        let bundle = fixture.bundle();
        let zip = write(&bundle).unwrap();
        assert_eq!(
            zip.file_name().and_then(|name| name.to_str()),
            Some("1234.scrawl.slim.zip")
        );
        assert_eq!(zip.parent(), Some(bundle.canonicalize().unwrap().as_path()));
        assert!(!bundle.join("1234.scrawl.slim.zip.tmp").exists());

        let entries = read_zip(&zip);
        let names: Vec<_> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "contact-sheet.png",
                "frames/frame-000.png",
                "frames/frame-001.png",
                "scrawl.json",
                "summary.md",
            ]
        );
        let summary = entries
            .iter()
            .find(|entry| entry.name == "summary.md")
            .unwrap();
        let text = std::str::from_utf8(&summary.data).unwrap();
        let absolute = bundle.canonicalize().unwrap().display().to_string();
        assert!(!text.contains(&absolute), "{text}");
        assert!(text.contains("[scrawl 0:02 · Display · .]"), "{text}");
        // The bundle keeps the summary it wrote.
        assert_eq!(
            fs::read_to_string(bundle.join("summary.md")).unwrap(),
            "stale"
        );
    }

    #[test]
    fn a_second_copy_replaces_the_first_without_taking_it_in() {
        let fixture = Fixture::new();
        let bundle = fixture.bundle();
        write(&bundle).unwrap();
        let zip = write(&bundle).unwrap();
        let entries = read_zip(&zip);
        assert!(
            entries
                .iter()
                .all(|entry| !entry.name.contains(".slim.zip"))
        );
        assert_eq!(entries.len(), 5);
    }
}
