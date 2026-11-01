//! Reads a GGUF file's header (metadata and tensor table) without loading
//! any weights, so an imported speech model is checked before transcribe.cpp
//! ever sees it. Every count and length is bounded: the file comes from the
//! person's disk and may be anything.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use color_eyre::eyre::{Result, bail, eyre};

const MAGIC: &[u8; 4] = b"GGUF";
const MAX_KEYS: u64 = 1 << 20;
const MAX_TENSORS: u64 = 1 << 20;
const MAX_STRING: u64 = 1 << 24;
const MAX_ARRAY: u64 = 1 << 26;
const MAX_DIMS: u32 = 8;
const DEFAULT_ALIGNMENT: u64 = 32;

/// The ggml tensor types every Hex build runs on Metal and on Vulkan (the
/// Linux and Windows builds embed Vulkan shaders for these alone).
pub(crate) const SUPPORTED_TENSOR_TYPES: [(u32, &str); 3] = [(0, "F32"), (1, "F16"), (8, "Q8_0")];

/// What an import needs from the header.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct GgufHeader {
    pub(crate) version: u32,
    pub(crate) strings: BTreeMap<String, String>,
    pub(crate) bools: BTreeMap<String, bool>,
    pub(crate) string_arrays: BTreeMap<String, Vec<String>>,
    /// Tensor count per ggml type id.
    pub(crate) tensor_types: BTreeMap<u32, usize>,
    /// Where the tensor data ends: the file must be at least this long.
    pub(crate) data_end: u64,
}

impl GgufHeader {
    pub(crate) fn string(&self, key: &str) -> Option<&str> {
        self.strings
            .get(key)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }

    pub(crate) fn bool(&self, key: &str) -> bool {
        self.bools.get(key).copied().unwrap_or(false)
    }

    /// Tensor types outside [`SUPPORTED_TENSOR_TYPES`], by name, with counts.
    pub(crate) fn unsupported_tensor_types(&self) -> Vec<String> {
        self.tensor_types
            .iter()
            .filter(|(kind, _)| !SUPPORTED_TENSOR_TYPES.iter().any(|(id, _)| id == *kind))
            .map(|(kind, count)| format!("{} ({count})", type_name(*kind)))
            .collect()
    }
}

/// Reads the header of the GGUF at `path`.
pub(crate) fn read(path: &Path) -> Result<GgufHeader> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut reader = Reader {
        inner: BufReader::new(file),
    };
    let header = reader.header()?;
    if header.data_end > length {
        bail!(
            "the file is cut short: its tensors need {} bytes, it has {length}",
            header.data_end
        );
    }
    Ok(header)
}

struct Reader<R> {
    inner: R,
}

impl<R: Read + Seek> Reader<R> {
    fn header(&mut self) -> Result<GgufHeader> {
        let mut magic = [0_u8; 4];
        self.inner
            .read_exact(&mut magic)
            .map_err(|_| eyre!("it is not a GGUF file"))?;
        if &magic != MAGIC {
            bail!("it is not a GGUF file");
        }
        let version = self.u32()?;
        if !(2..=3).contains(&version) {
            bail!("it is GGUF version {version}; Hex reads versions 2 and 3");
        }
        let tensors = self.u64()?;
        let keys = self.u64()?;
        if tensors > MAX_TENSORS || keys > MAX_KEYS {
            bail!("its header is too large to be a speech model");
        }
        let mut header = GgufHeader {
            version,
            ..Default::default()
        };
        let mut alignment = DEFAULT_ALIGNMENT;
        for _ in 0..keys {
            let key = self.string()?;
            let kind = self.u32()?;
            match kind {
                8 => {
                    let value = self.string()?;
                    header.strings.insert(key, value);
                }
                7 => {
                    let value = self.u8()? != 0;
                    header.bools.insert(key, value);
                }
                4 if key == "general.alignment" => {
                    alignment = u64::from(self.u32()?);
                }
                9 => {
                    let item = self.u32()?;
                    let count = self.u64()?;
                    if count > MAX_ARRAY {
                        bail!("its metadata {key} is too long");
                    }
                    if item == 8 {
                        let mut values = Vec::new();
                        for _ in 0..count {
                            values.push(self.string()?);
                        }
                        header.string_arrays.insert(key, values);
                    } else {
                        for _ in 0..count {
                            self.skip_value(item)?;
                        }
                    }
                }
                kind => self.skip_value(kind)?,
            }
        }
        if alignment == 0 || !alignment.is_power_of_two() {
            bail!("its general.alignment ({alignment}) is invalid");
        }
        let mut extent = 0_u64;
        for _ in 0..tensors {
            let name = self.string()?;
            let dimensions = self.u32()?;
            if dimensions == 0 || dimensions > MAX_DIMS {
                bail!("tensor {name} has {dimensions} dimensions");
            }
            let mut elements = 1_u64;
            for _ in 0..dimensions {
                elements = elements
                    .checked_mul(self.u64()?)
                    .ok_or_else(|| eyre!("tensor {name} is too large"))?;
            }
            let kind = self.u32()?;
            let offset = self.u64()?;
            *header.tensor_types.entry(kind).or_default() += 1;
            if let Some(bytes) = tensor_bytes(kind, elements) {
                extent = extent.max(
                    offset
                        .checked_add(bytes)
                        .ok_or_else(|| eyre!("tensor {name} is out of range"))?,
                );
            }
        }
        let position = self.inner.stream_position()?;
        let data_start = position.div_ceil(alignment) * alignment;
        header.data_end = data_start + extent;
        Ok(header)
    }

    fn skip_value(&mut self, kind: u32) -> Result<()> {
        let width = match kind {
            0 | 1 | 7 => 1,
            2 | 3 => 2,
            4..=6 => 4,
            10..=12 => 8,
            8 => {
                let length = self.u64()?;
                if length > MAX_STRING {
                    bail!("its metadata holds a string too long to read");
                }
                return self.skip(length);
            }
            9 => {
                let item = self.u32()?;
                let count = self.u64()?;
                if count > MAX_ARRAY {
                    bail!("its metadata holds an array too long to read");
                }
                for _ in 0..count {
                    self.skip_value(item)?;
                }
                return Ok(());
            }
            kind => bail!("its metadata has an unknown value type {kind}"),
        };
        self.skip(width)
    }

    fn skip(&mut self, bytes: u64) -> Result<()> {
        let skip = i64::try_from(bytes).map_err(|_| eyre!("its header is corrupt"))?;
        self.inner.seek(SeekFrom::Current(skip))?;
        Ok(())
    }

    fn string(&mut self) -> Result<String> {
        let length = self.u64()?;
        if length > MAX_STRING {
            bail!("its header holds a string too long to read");
        }
        let mut bytes = vec![0_u8; usize::try_from(length)?];
        self.inner
            .read_exact(&mut bytes)
            .map_err(|_| eyre!("its header is cut short"))?;
        String::from_utf8(bytes).map_err(|_| eyre!("its header holds text that isn't UTF-8"))
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut bytes = [0_u8; N];
        self.inner
            .read_exact(&mut bytes)
            .map_err(|_| eyre!("its header is cut short"))?;
        Ok(bytes)
    }
}

/// Bytes a tensor of `kind` with `elements` takes, for the types Hex runs.
fn tensor_bytes(kind: u32, elements: u64) -> Option<u64> {
    match kind {
        0 => elements.checked_mul(4),
        1 => elements.checked_mul(2),
        // 32 values in 34 bytes: an f16 scale and 32 int8s.
        8 => elements.div_ceil(32).checked_mul(34),
        _ => None,
    }
}

fn type_name(kind: u32) -> String {
    const NAMES: [&str; 31] = [
        "F32", "F16", "Q4_0", "Q4_1", "Q4_2", "Q4_3", "Q5_0", "Q5_1", "Q8_0", "Q8_1", "Q2_K",
        "Q3_K", "Q4_K", "Q5_K", "Q6_K", "Q8_K", "IQ2_XXS", "IQ2_XS", "IQ3_XXS", "IQ1_S", "IQ4_NL",
        "IQ3_S", "IQ2_S", "IQ4_XS", "I8", "I16", "I32", "I64", "F64", "IQ1_M", "BF16",
    ];
    NAMES
        .get(kind as usize)
        .map_or_else(|| format!("type {kind}"), |name| (*name).to_owned())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal GGUF: string metadata, string-array metadata, bools, and
    /// one tensor per `(name, type, elements)`, with the data it needs.
    pub(crate) fn gguf(
        strings: &[(&str, &str)],
        arrays: &[(&str, &[&str])],
        bools: &[(&str, bool)],
        tensors: &[(&str, u32, u64)],
    ) -> Vec<u8> {
        fn string(out: &mut Vec<u8>, text: &str) {
            out.extend((text.len() as u64).to_le_bytes());
            out.extend(text.as_bytes());
        }
        let mut out = Vec::new();
        out.extend(MAGIC);
        out.extend(3_u32.to_le_bytes());
        out.extend((tensors.len() as u64).to_le_bytes());
        out.extend(((strings.len() + arrays.len() + bools.len() + 1) as u64).to_le_bytes());
        // An unread u64 value the reader has to skip.
        string(&mut out, "general.file_type");
        out.extend(10_u32.to_le_bytes());
        out.extend(7_u64.to_le_bytes());
        for (key, value) in strings {
            string(&mut out, key);
            out.extend(8_u32.to_le_bytes());
            string(&mut out, value);
        }
        for (key, values) in arrays {
            string(&mut out, key);
            out.extend(9_u32.to_le_bytes());
            out.extend(8_u32.to_le_bytes());
            out.extend((values.len() as u64).to_le_bytes());
            for value in *values {
                string(&mut out, value);
            }
        }
        for (key, value) in bools {
            string(&mut out, key);
            out.extend(7_u32.to_le_bytes());
            out.push(u8::from(*value));
        }
        let mut offset = 0_u64;
        for (name, kind, elements) in tensors {
            string(&mut out, name);
            out.extend(1_u32.to_le_bytes());
            out.extend(elements.to_le_bytes());
            out.extend(kind.to_le_bytes());
            out.extend(offset.to_le_bytes());
            offset += tensor_bytes(*kind, *elements).unwrap_or(4).div_ceil(32) * 32;
        }
        out.resize(out.len().div_ceil(32) * 32 + offset as usize, 0);
        out
    }

    pub(crate) fn read_header(bytes: Vec<u8>) -> GgufHeader {
        read_bytes(bytes).unwrap()
    }

    fn read_bytes(bytes: Vec<u8>) -> Result<GgufHeader> {
        let length = bytes.len() as u64;
        let header = Reader {
            inner: std::io::Cursor::new(bytes),
        }
        .header()?;
        if header.data_end > length {
            bail!("cut short");
        }
        Ok(header)
    }

    #[test]
    fn metadata_and_tensor_types_are_read_without_the_weights() {
        let header = read_bytes(gguf(
            &[
                ("general.architecture", "whisper"),
                ("stt.variant", "whisper-small"),
            ],
            &[("general.languages", &["en", "de"])],
            &[("stt.capability.lang_detect", true)],
            &[("a", 0, 8), ("b", 1, 8), ("c", 8, 64)],
        ))
        .unwrap();
        assert_eq!(header.string("general.architecture"), Some("whisper"));
        assert_eq!(header.string_arrays["general.languages"], ["en", "de"]);
        assert!(header.bool("stt.capability.lang_detect"));
        assert!(!header.bool("stt.capability.streaming"));
        assert_eq!(header.tensor_types, [(0, 1), (1, 1), (8, 1)].into());
        assert!(header.unsupported_tensor_types().is_empty());
    }

    #[test]
    fn other_tensor_types_are_named() {
        let header = read_bytes(gguf(&[], &[], &[], &[("a", 12, 256), ("b", 12, 256)])).unwrap();
        assert_eq!(header.unsupported_tensor_types(), ["Q4_K (2)"]);
    }

    #[test]
    fn junk_and_truncated_files_are_refused() {
        let junk = read_bytes(b"PK\x03\x04 not a model".to_vec()).unwrap_err();
        assert_eq!(junk.to_string(), "it is not a GGUF file");
        let mut cut = gguf(
            &[("general.architecture", "parakeet")],
            &[],
            &[],
            &[("a", 0, 1024)],
        );
        cut.truncate(cut.len() - 100);
        assert!(read_bytes(cut).is_err());
        let mut header = gguf(&[("general.architecture", "parakeet")], &[], &[], &[]);
        header.truncate(30);
        assert_eq!(
            read_bytes(header).unwrap_err().to_string(),
            "its header is cut short"
        );
    }
}
