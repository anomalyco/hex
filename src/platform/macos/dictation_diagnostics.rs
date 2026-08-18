use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use color_eyre::eyre::Result;
use hound::{SampleFormat, WavSpec, WavWriter};

const RETENTION_ENV: &str = "HEX_RETAIN_DICTATION_AUDIO";
const SAMPLE_RATE: u32 = 16_000;

pub fn persist(samples: &[f32]) {
    let Some(retention) = retention() else {
        return;
    };
    let directory = match crate::app_paths::support_dir() {
        Ok(path) => path.join("diagnostic-dictation-audio"),
        Err(error) => {
            tracing::warn!(%error, "could not locate diagnostic dictation audio directory");
            return;
        }
    };
    if let Err(error) = persist_at(&directory, samples, retention) {
        tracing::warn!(%error, "could not retain diagnostic dictation audio");
    }
}

fn retention() -> Option<usize> {
    std::env::var(RETENTION_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|retention| *retention > 0)
}

fn persist_at(directory: &Path, samples: &[f32], retention: usize) -> Result<()> {
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let timestamp_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let path = directory.join(format!("dictation-{timestamp_ms}.wav"));
    let mut writer = WavWriter::create(
        &path,
        WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        },
    )?;
    for sample in samples {
        writer.write_sample(*sample)?;
    }
    writer.finalize()?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    rotate(directory, retention)?;
    tracing::info!(path = %path.display(), "retained diagnostic dictation audio");
    Ok(())
}

fn rotate(directory: &Path, retention: usize) -> Result<()> {
    let mut recordings = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("wav"))
        .collect::<Vec<_>>();
    recordings.sort();
    let remove = recordings.len().saturating_sub(retention);
    for path in recordings.into_iter().take(remove) {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_recordings_are_owner_only_and_bounded() {
        let directory =
            std::env::temp_dir().join(format!("hex-dictation-diagnostics-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        persist_at(&directory, &[0.25; 16_000], 1).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        persist_at(&directory, &[0.5; 16_000], 1).unwrap();

        let files = fs::read_dir(&directory).unwrap().collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0]
                .as_ref()
                .unwrap()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
