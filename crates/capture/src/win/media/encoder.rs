//! GPU-surface sink-writer input. A tracked sample leases each texture until MF releases it.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use windows::Win32::Graphics::Direct3D11::{D3D11_BIND_RENDER_TARGET, ID3D11Texture2D};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Variant::VARIANT;
use windows::core::{HSTRING, Interface};

use super::{Gpu, MAX_FPS, frame_time, video_type};
use crate::log::scrawl_log;

/// Windows recording quality. Annotate uses Standard; the native-resolution presets are
/// available to capture-crate callers only, with no persisted setting or UI selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    #[default]
    Standard,
    High,
    Master,
}

impl Quality {
    fn bitrate(self, width: u32, height: u32) -> (u32, u32) {
        let targets = match self {
            Self::Standard => [12, 24, 50],
            Self::High => [24, 40, 80],
            Self::Master => [40, 70, 120],
        };
        let peaks = match self {
            Self::Master => [80, 140, 200],
            _ => targets.map(|v| v * 2),
        };
        // Interpolate by pixel rate between the 1080p60, 1440p60 and 2160p60 anchors.
        let rate = u64::from(width) * u64::from(height) * u64::from(MAX_FPS);
        let anchors = [1920 * 1080 * 60_u64, 2560 * 1440 * 60, 3840 * 2160 * 60];
        let scale = |values: [u64; 3]| -> u32 {
            let bps = if rate <= anchors[0] {
                values[0] * 1_000_000 * rate / anchors[0]
            } else if rate >= anchors[2] {
                values[2] * 1_000_000 * rate / anchors[2]
            } else {
                let i = usize::from(rate > anchors[1]);
                values[i] * 1_000_000
                    + (values[i + 1] - values[i]) * 1_000_000 * (rate - anchors[i])
                        / (anchors[i + 1] - anchors[i])
            };
            bps.clamp(128_000, u64::from(u32::MAX)) as u32
        };
        (scale(targets), scale(peaks))
    }
}

fn level(width: u32, height: u32, peak: u32) -> Result<u32> {
    let blocks = u64::from(width.div_ceil(16)) * u64::from(height.div_ceil(16));
    // Annex A: MaxFS, MaxMBPS and High-profile MaxBR (bits/s).
    for (level, fs, mbps, bitrate) in [
        (42, 8704, 522_240, 62_500_000),
        (50, 22_080, 589_824, 168_750_000),
        (51, 36_864, 983_040, 300_000_000),
        (52, 36_864, 2_073_600, 300_000_000),
        (60, 139_264, 4_177_920, 300_000_000),
        (61, 139_264, 8_355_840, 600_000_000),
        (62, 139_264, 16_711_680, 1_000_000_000),
    ] {
        if blocks <= fs && blocks * u64::from(MAX_FPS) <= mbps && peak <= bitrate {
            return Ok(level);
        }
    }
    anyhow::bail!("Resolution/bitrate exceeds H.264 level 6.2")
}

pub(super) struct Encoder {
    writer: IMFSinkWriter,
    stream: u32,
    _manager: IMFDXGIDeviceManager,
    allocator: IMFVideoSampleAllocatorEx,
}

impl Encoder {
    pub(super) fn new(
        gpu: &Gpu,
        path: &Path,
        width: u32,
        height: u32,
        quality: Quality,
    ) -> Result<Self> {
        let (bitrate, peak) = quality.bitrate(width, height);
        let level = level(width, height, peak)?;
        let mut manager = None;
        let mut token = 0;
        unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager) }?;
        let manager = manager.context("DXGI device manager")?;
        unsafe { manager.ResetDevice(&gpu.device, token) }?;
        let mut attributes = None;
        unsafe { MFCreateAttributes(&mut attributes, 4) }?;
        let attributes = attributes.context("writer attributes")?;
        unsafe {
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            attributes.SetUINT32(&MF_READWRITE_D3D_OPTIONAL, 0)?;
            attributes.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &manager)?;
            attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
        }
        let writer = unsafe {
            MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None, &attributes)
        }?;
        let output = video_type(&MFVideoFormat_H264, width, height)?;
        let input = video_type(&MFVideoFormat_NV12, width, height)?;
        for media in [&output, &input] {
            unsafe {
                media.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?;
                media.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
                media.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
                media.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
            }
        }
        unsafe {
            output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            output.SetUINT32(&MF_MT_MPEG2_PROFILE, 100)?;
            output.SetUINT32(&MF_MT_MPEG2_LEVEL, level)?;
            input.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        }
        let stream = unsafe { writer.AddStream(&output) }?;
        unsafe { writer.SetInputMediaType(stream, &input, None) }?;
        let extended = writer.cast::<IMFSinkWriterEx>()?;
        let mut transform = None;
        unsafe { extended.GetTransformForStream(stream, 0, None, &mut transform) }?;
        let transform = transform.context("sink writer encoder")?;
        let codec = transform.cast::<ICodecAPI>()?;
        let settings = [
            (
                "rate_control",
                CODECAPI_AVEncCommonRateControlMode,
                eAVEncCommonRateControlMode_PeakConstrainedVBR.0 as u32,
            ),
            ("mean_bitrate", CODECAPI_AVEncCommonMeanBitRate, bitrate),
            ("peak_bitrate", CODECAPI_AVEncCommonMaxBitRate, peak),
            ("b_frames", CODECAPI_AVEncMPVDefaultBPictureCount, 0),
            ("gop", CODECAPI_AVEncMPVGOPSize, MAX_FPS),
        ];
        for (name, key, value) in settings {
            unsafe { codec.SetValue(&key, &VARIANT::from(value)) }
                .with_context(|| format!("Encoder setting {name}={value}"))?;
        }
        unsafe { writer.BeginWriting() }?;
        let accepted = unsafe { transform.GetOutputCurrentType(0) }?;
        let profile = unsafe { accepted.GetUINT32(&MF_MT_MPEG2_PROFILE) }?;
        let accepted_level = unsafe { accepted.GetUINT32(&MF_MT_MPEG2_LEVEL) }?;
        ensure!(
            profile == 100 && accepted_level == level,
            "Encoder refused High level {level}: profile={profile} level={accepted_level}"
        );
        scrawl_log(format!(
            "Encoder {quality:?}: {width}x{height}@60 High level={accepted_level} NV12 BT.709 limited; DXGI surfaces, pool=8"
        ));
        for (name, key, wanted) in settings {
            let value = unsafe { codec.GetValue(&key) }?;
            let value = u32::try_from(&value)?;
            scrawl_log(format!(
                "Encoder accepted {name}={value} (requested {wanted})"
            ));
            ensure!(
                value == wanted,
                "Encoder changed {name}: {wanted} -> {value}"
            );
        }
        let mut raw = std::ptr::null_mut();
        unsafe { MFCreateVideoSampleAllocatorEx(&IMFVideoSampleAllocatorEx::IID, &mut raw) }?;
        let allocator = unsafe { IMFVideoSampleAllocatorEx::from_raw(raw) };
        let mut pool_attributes = None;
        unsafe { MFCreateAttributes(&mut pool_attributes, 1) }?;
        let pool_attributes = pool_attributes.context("sample pool attributes")?;
        unsafe {
            pool_attributes.SetUINT32(&MF_SA_D3D11_BINDFLAGS, D3D11_BIND_RENDER_TARGET.0 as u32)?;
            allocator.SetDirectXManager(&manager)?;
            allocator.InitializeSampleAllocatorEx(8, 8, &pool_attributes, &input)?;
        }
        Ok(Self {
            writer,
            stream,
            _manager: manager,
            allocator,
        })
    }

    pub(super) fn write(&mut self, gpu: &Gpu, source: &ID3D11Texture2D, index: u64) -> Result<()> {
        // MF's allocator owns the tracked-sample callback. AllocateSample never returns an
        // in-flight sample, and the equal initial/maximum counts prevent the pool growing.
        let sample = unsafe { self.allocator.AllocateSample() }
            .context("Encoder has not released any of its eight GPU surfaces")?;
        let original = unsafe { sample.GetBufferByIndex(0) }?;
        let surface = original.cast::<IMFDXGIBuffer>()?;
        let mut raw = std::ptr::null_mut();
        unsafe { surface.GetResource(&ID3D11Texture2D::IID, &mut raw) }?;
        let texture = unsafe { ID3D11Texture2D::from_raw(raw) };
        let subresource = unsafe { surface.GetSubresourceIndex() }?;
        let buffer = unsafe {
            MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &texture, subresource, false)
        }?;
        unsafe {
            sample.RemoveAllBuffers()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(frame_time(index))?;
            sample.SetSampleDuration(frame_time(index + 1) - frame_time(index))?;
        }
        unsafe {
            // The VP's output is reusable immediately; the pooled copy stays immutable until
            // the tracked sample is released. All commands use this same protected device.
            gpu.context
                .CopySubresourceRegion(&texture, subresource, 0, 0, 0, source, 0, None);
            gpu.context.Flush();
            self.writer.WriteSample(self.stream, &sample)?;
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<()> {
        unsafe { self.writer.Finalize() }?;
        Ok(())
    }
}
