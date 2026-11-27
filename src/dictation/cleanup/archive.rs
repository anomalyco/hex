//! Unpacks the cleanup model's zip archive, with no archive dependency and
//! no `unzip`: the central directory is read by hand, and an entry is either
//! stored or deflated.
//!
//! The archive is only unpacked after its SHA-256 matched the pinned one, so
//! these checks are a second line. Every entry must sit inside the one
//! folder the archive is known to hold, as a plain file or folder: a name
//! that is absolute, climbs with `..`, or uses a backslash or a drive
//! letter is refused, and so is a symbolic link. Nothing is written past an
//! entry's declared size, past the caller's limits, or over a file that is
//! already there, and each file's CRC-32 is checked.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use color_eyre::eyre::{Result, bail, ensure, eyre};

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL: u32 = 0x0605_4b50;
const LOCAL_LENGTH: usize = 30;
const CENTRAL_LENGTH: usize = 46;
const END_LENGTH: usize = 22;
/// The end record's comment is at most this long.
const LONGEST_COMMENT: usize = u16::MAX as usize;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
/// Flag bit 0: the entry is encrypted.
const ENCRYPTED: u16 = 1;
/// `version made by`'s high byte for Unix, whose external attributes hold
/// the file's mode.
const UNIX: u16 = 3;
const FILE_TYPE: u32 = 0o170_000;
const REGULAR_FILE: u32 = 0o100_000;
const DIRECTORY: u32 = 0o040_000;

/// The most an archive may hold.
#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    pub(super) entries: usize,
    /// Unpacked bytes, all files together.
    pub(super) bytes: u64,
}

struct Entry {
    path: PathBuf,
    directory: bool,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    local_header: u64,
    name: Vec<u8>,
}

/// Unpacks `archive` into `destination`, an empty folder the caller made.
/// Every entry must sit under `root`, the one folder the archive holds. A
/// refused archive may leave some of its files behind in `destination`,
/// which the caller throws away.
pub(super) fn unpack(archive: &Path, destination: &Path, root: &str, limits: Limits) -> Result<()> {
    let mut file = File::open(archive)?;
    let entries = entries(&mut file, root, limits)?;
    for entry in &entries {
        let path = destination.join(&entry.path);
        if entry.directory {
            fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write(&mut file, entry, &path)?;
    }
    Ok(())
}

/// The archive's entries, each one checked before anything is written.
fn entries(file: &mut File, root: &str, limits: Limits) -> Result<Vec<Entry>> {
    let length = file.metadata()?.len();
    let tail_length = length.min((END_LENGTH + LONGEST_COMMENT) as u64);
    let mut tail = vec![0; tail_length as usize];
    file.seek(SeekFrom::Start(length - tail_length))?;
    file.read_exact(&mut tail)?;
    let end = (0..=tail.len().saturating_sub(END_LENGTH))
        .rev()
        .find(|&at| tail.len() >= END_LENGTH && u32_at(&tail, at) == END_OF_CENTRAL)
        .ok_or_else(|| eyre!("the archive has no directory"))?;
    let end = &tail[end..];
    let count = usize::from(u16_at(end, 10));
    let (directory_length, directory_start) = (u32_at(end, 12), u32_at(end, 16));
    ensure!(
        u16_at(end, 4) == 0 && u16_at(end, 6) == 0 && u16_at(end, 8) == u16_at(end, 10),
        "the archive spans several disks"
    );
    // A zip64 archive marks these fields full; the model is far smaller.
    ensure!(
        count != usize::from(u16::MAX)
            && directory_length != u32::MAX
            && directory_start != u32::MAX,
        "the archive is too large"
    );
    ensure!(
        count <= limits.entries,
        "the archive holds {count} entries, more than {}",
        limits.entries
    );
    ensure!(
        u64::from(directory_start) + u64::from(directory_length) <= length,
        "the archive's directory is cut short"
    );
    let mut directory = vec![0; directory_length as usize];
    file.seek(SeekFrom::Start(u64::from(directory_start)))?;
    file.read_exact(&mut directory)?;

    let mut entries = Vec::with_capacity(count);
    let mut at = 0;
    let mut total = 0_u64;
    for _ in 0..count {
        ensure!(
            directory.len() >= at + CENTRAL_LENGTH && u32_at(&directory, at) == CENTRAL_HEADER,
            "the archive's directory is damaged"
        );
        let header = &directory[at..];
        let name_length = usize::from(u16_at(header, 28));
        let after_name = CENTRAL_LENGTH + name_length;
        let next = after_name + usize::from(u16_at(header, 30)) + usize::from(u16_at(header, 32));
        ensure!(header.len() >= next, "the archive's directory is damaged");
        let name = &header[CENTRAL_LENGTH..after_name];
        let text = std::str::from_utf8(name)
            .map_err(|_| eyre!("the archive names a file in no known encoding"))?;
        let path = path_under(text, root)
            .ok_or_else(|| eyre!("the archive holds a file outside {root}"))?;
        let directory_entry = text.ends_with('/');
        let (made_by, flags, method) = (u16_at(header, 4), u16_at(header, 8), u16_at(header, 10));
        let mode = u32_at(header, 38) >> 16;
        let kind = mode & FILE_TYPE;
        let plain = made_by >> 8 != UNIX
            || kind == 0
            || kind
                == if directory_entry {
                    DIRECTORY
                } else {
                    REGULAR_FILE
                };
        ensure!(
            plain,
            "the archive holds something other than files and folders"
        );
        ensure!(flags & ENCRYPTED == 0, "the archive is encrypted");
        ensure!(
            matches!(method, STORED | DEFLATED),
            "the archive is packed in a way Hex does not read"
        );
        let (compressed, size) = (u64::from(u32_at(header, 20)), u64::from(u32_at(header, 24)));
        ensure!(
            !directory_entry || size == 0,
            "the archive's directory is damaged"
        );
        total += size;
        ensure!(
            total <= limits.bytes,
            "the archive unpacks to more than {} bytes",
            limits.bytes
        );
        entries.push(Entry {
            path,
            directory: directory_entry,
            method,
            crc: u32_at(header, 16),
            compressed,
            size,
            local_header: u64::from(u32_at(header, 42)),
            name: name.to_vec(),
        });
        at += next;
    }
    Ok(entries)
}

/// Where an entry named `name` goes, relative to the destination: under
/// `root`, through plain names only.
fn path_under(name: &str, root: &str) -> Option<PathBuf> {
    if name.starts_with('/') || name.contains(['\\', '\0', ':']) {
        return None;
    }
    let mut parts = name.strip_suffix('/').unwrap_or(name).split('/');
    if parts.next() != Some(root) {
        return None;
    }
    let mut path = PathBuf::from(root);
    for part in parts {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        path.push(part);
    }
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then_some(path)
}

/// Writes one file's bytes to `path`, which must not exist.
fn write(archive: &mut File, entry: &Entry, path: &Path) -> Result<()> {
    let mut header = [0; LOCAL_LENGTH];
    archive.seek(SeekFrom::Start(entry.local_header))?;
    archive.read_exact(&mut header)?;
    ensure!(
        u32_at(&header, 0) == LOCAL_HEADER,
        "the archive's files are damaged"
    );
    let mut name = vec![0; usize::from(u16_at(&header, 26))];
    archive.read_exact(&mut name)?;
    ensure!(
        name == entry.name,
        "the archive names a file two different ways"
    );
    archive.seek(SeekFrom::Current(i64::from(u16_at(&header, 28))))?;
    let packed = archive.take(entry.compressed);
    let mut unpacked: Box<dyn Read + '_> = if entry.method == STORED {
        Box::new(packed)
    } else {
        Box::new(flate2::read::DeflateDecoder::new(packed))
    };
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut checksum = flate2::Crc::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut written = 0_u64;
    loop {
        let read = unpacked.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        written += read as u64;
        if written > entry.size {
            bail!("the archive holds more than it says");
        }
        checksum.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
    }
    ensure!(
        written == entry.size && checksum.sum() == entry.crc,
        "the archive's files are damaged"
    );
    file.sync_all()?;
    Ok(())
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        entries: 16,
        bytes: 1 << 20,
    };

    /// One entry of a test archive.
    pub(in crate::dictation::cleanup) struct Packed {
        pub(in crate::dictation::cleanup) name: Vec<u8>,
        pub(in crate::dictation::cleanup) contents: Vec<u8>,
        deflate: bool,
        /// The Unix mode in the external attributes.
        mode: u32,
        /// The size the directory declares, when it lies.
        declared: Option<u32>,
        crc: Option<u32>,
        flags: u16,
    }

    pub(in crate::dictation::cleanup) fn file(name: &str, contents: &[u8]) -> Packed {
        Packed {
            name: name.as_bytes().to_vec(),
            contents: contents.to_vec(),
            deflate: true,
            mode: REGULAR_FILE | 0o644,
            declared: None,
            crc: None,
            flags: 0,
        }
    }

    fn folder(name: &str) -> Packed {
        Packed {
            deflate: false,
            mode: DIRECTORY | 0o755,
            ..file(name, b"")
        }
    }

    /// A zip archive of `entries`, as `zip` writes one.
    pub(in crate::dictation::cleanup) fn archive(entries: &[Packed]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for entry in entries {
            let packed = if entry.deflate {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(&entry.contents).unwrap();
                encoder.finish().unwrap()
            } else {
                entry.contents.clone()
            };
            let mut checksum = flate2::Crc::new();
            checksum.update(&entry.contents);
            let crc = entry.crc.unwrap_or(checksum.sum());
            let size = entry.declared.unwrap_or(entry.contents.len() as u32);
            let method = if entry.deflate { DEFLATED } else { STORED };
            let offset = out.len() as u32;
            let mut shared = Vec::new();
            shared.extend(20_u16.to_le_bytes());
            shared.extend(entry.flags.to_le_bytes());
            shared.extend(method.to_le_bytes());
            shared.extend([0; 4]);
            shared.extend(crc.to_le_bytes());
            shared.extend((packed.len() as u32).to_le_bytes());
            shared.extend(size.to_le_bytes());
            shared.extend((entry.name.len() as u16).to_le_bytes());
            shared.extend(0_u16.to_le_bytes());

            out.extend(LOCAL_HEADER.to_le_bytes());
            out.extend(&shared);
            out.extend(&entry.name);
            out.extend(&packed);

            directory.extend(CENTRAL_HEADER.to_le_bytes());
            directory.extend(((UNIX << 8) | 20).to_le_bytes());
            directory.extend(&shared);
            directory.extend([0; 6]);
            directory.extend((entry.mode << 16).to_le_bytes());
            directory.extend(offset.to_le_bytes());
            directory.extend(&entry.name);
        }
        let start = out.len() as u32;
        out.extend(&directory);
        out.extend(END_OF_CENTRAL.to_le_bytes());
        out.extend([0; 4]);
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((directory.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend(0_u16.to_le_bytes());
        out
    }

    /// A scratch folder, removed when the test ends.
    pub(in crate::dictation::cleanup) struct Scratch(pub(in crate::dictation::cleanup) PathBuf);

    impl Scratch {
        pub(in crate::dictation::cleanup) fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "hex-cleanup-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Unpacks `entries` into a fresh folder and says what became of it.
    fn unpacked(name: &str, entries: &[Packed], limits: Limits) -> (Scratch, Result<()>) {
        let scratch = Scratch::new(name);
        let zip = scratch.0.join("model.zip");
        fs::write(&zip, archive(entries)).unwrap();
        let destination = scratch.0.join("out");
        fs::create_dir_all(&destination).unwrap();
        let result = unpack(&zip, &destination, "Model.mlpackage", limits);
        (scratch, result)
    }

    /// Every file under `folder`, by its path from it.
    fn files(folder: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut pending = vec![folder.to_path_buf()];
        while let Some(next) = pending.pop() {
            for entry in fs::read_dir(next).unwrap().flatten() {
                if entry.file_type().unwrap().is_dir() {
                    pending.push(entry.path());
                } else {
                    let path = entry.path();
                    let relative = path.strip_prefix(folder).unwrap();
                    found.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        found.sort();
        found
    }

    #[test]
    fn a_model_package_unpacks_with_its_folders_and_bytes() {
        let weights = vec![7_u8; 200_000];
        let stored = Packed {
            deflate: false,
            ..file("Model.mlpackage/Data/model.mlmodel", b"stored as is")
        };
        let (scratch, result) = unpacked(
            "whole",
            &[
                folder("Model.mlpackage/"),
                file(
                    "Model.mlpackage/Manifest.json",
                    b"{\"itemInfoEntries\": {}}",
                ),
                folder("Model.mlpackage/Data/"),
                folder("Model.mlpackage/Data/weights/"),
                file("Model.mlpackage/Data/weights/weight.bin", &weights),
                stored,
            ],
            LIMITS,
        );
        result.unwrap();
        let out = scratch.0.join("out");
        assert_eq!(
            files(&out),
            [
                "Model.mlpackage/Data/model.mlmodel",
                "Model.mlpackage/Data/weights/weight.bin",
                "Model.mlpackage/Manifest.json",
            ]
        );
        assert_eq!(
            fs::read(out.join("Model.mlpackage/Data/weights/weight.bin")).unwrap(),
            weights
        );
        assert_eq!(
            fs::read(out.join("Model.mlpackage/Data/model.mlmodel")).unwrap(),
            b"stored as is"
        );
    }

    /// Zip slip: no name writes outside the folder the archive is for.
    #[test]
    fn a_file_that_would_land_outside_the_package_is_refused() {
        for name in [
            "../evil",
            "Model.mlpackage/../../evil",
            "Model.mlpackage/./evil",
            "Model.mlpackage//evil",
            "/tmp/evil",
            "Model.mlpackage\\..\\evil",
            "C:/evil",
            "Model.mlpackage/C:evil",
            "Other.mlpackage/evil",
            "evil",
            "Model.mlpackageevil/file",
            "",
            "Model.mlpackage/nul\0byte",
        ] {
            let (scratch, result) = unpacked("slip", &[file(name, b"evil")], LIMITS);
            let error = result.expect_err(name).to_string();
            assert_eq!(
                error, "the archive holds a file outside Model.mlpackage",
                "{name:?}"
            );
            assert_eq!(files(&scratch.0), ["model.zip"], "{name:?}");
        }
        assert_eq!(
            path_under("Model.mlpackage/Data/weights/weight.bin", "Model.mlpackage"),
            Some(PathBuf::from("Model.mlpackage/Data/weights/weight.bin"))
        );
        assert_eq!(
            path_under("Model.mlpackage/", "Model.mlpackage"),
            Some(PathBuf::from("Model.mlpackage"))
        );
    }

    #[test]
    fn a_symbolic_link_or_an_unreadable_entry_is_refused() {
        let link = Packed {
            mode: 0o120_777,
            deflate: false,
            ..file("Model.mlpackage/link", b"/etc/passwd")
        };
        let encrypted = Packed {
            flags: ENCRYPTED,
            ..file("Model.mlpackage/secret", b"secret")
        };
        let invalid_name = Packed {
            name: b"Model.mlpackage/\xff".to_vec(),
            ..file("", b"bytes")
        };
        for (entry, why) in [
            (
                link,
                "the archive holds something other than files and folders",
            ),
            (encrypted, "the archive is encrypted"),
            (
                invalid_name,
                "the archive names a file in no known encoding",
            ),
        ] {
            let (scratch, result) = unpacked("kinds", &[entry], LIMITS);
            assert_eq!(result.unwrap_err().to_string(), why);
            assert_eq!(files(&scratch.0), ["model.zip"]);
        }
    }

    #[test]
    fn an_archive_larger_than_it_says_or_than_allowed_is_refused() {
        let bomb = Packed {
            declared: Some(10),
            ..file("Model.mlpackage/bomb", &vec![0_u8; 300_000])
        };
        let (_scratch, result) = unpacked("bomb", &[bomb], LIMITS);
        assert_eq!(
            result.unwrap_err().to_string(),
            "the archive holds more than it says"
        );

        let (_scratch, result) = unpacked(
            "bytes",
            &[
                file("Model.mlpackage/a", &[1; 600]),
                file("Model.mlpackage/b", &[2; 600]),
            ],
            Limits {
                entries: 16,
                bytes: 1_000,
            },
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "the archive unpacks to more than 1000 bytes"
        );

        let many: Vec<Packed> = (0..5)
            .map(|index| file(&format!("Model.mlpackage/{index}"), b"x"))
            .collect();
        let (_scratch, result) = unpacked(
            "entries",
            &many,
            Limits {
                entries: 4,
                bytes: 1_000,
            },
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "the archive holds 5 entries, more than 4"
        );
    }

    #[test]
    fn damaged_bytes_and_a_file_named_twice_are_refused() {
        let damaged = Packed {
            crc: Some(1),
            ..file("Model.mlpackage/weights", b"weights")
        };
        let (_scratch, result) = unpacked("crc", &[damaged], LIMITS);
        assert_eq!(
            result.unwrap_err().to_string(),
            "the archive's files are damaged"
        );

        let short = Packed {
            declared: Some(100),
            ..file("Model.mlpackage/weights", b"weights")
        };
        let (_scratch, result) = unpacked("short", &[short], LIMITS);
        assert_eq!(
            result.unwrap_err().to_string(),
            "the archive's files are damaged"
        );

        let (_scratch, result) = unpacked(
            "twice",
            &[
                file("Model.mlpackage/weights", b"first"),
                file("Model.mlpackage/weights", b"second"),
            ],
            LIMITS,
        );
        assert!(result.is_err());

        let scratch = Scratch::new("garbage");
        let zip = scratch.0.join("model.zip");
        fs::write(&zip, b"not a zip archive at all").unwrap();
        let result = unpack(&zip, &scratch.0, "Model.mlpackage", LIMITS);
        assert_eq!(
            result.unwrap_err().to_string(),
            "the archive has no directory"
        );
        fs::write(&zip, b"").unwrap();
        assert!(unpack(&zip, &scratch.0, "Model.mlpackage", LIMITS).is_err());
    }
}
