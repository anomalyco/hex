# Repeated HEVC recordings release native surfaces

The opt-in `video_writer::tests::headless_encoder_memory` probe creates 51
half-second, 640×360 HEVC movies on one Rust thread, using synthetic
CMSampleBuffers. It never opens a screen stream or microphone. Each movie is
deleted after it finishes. `footprint` and `vmmap -summary` sample every ten
cycles; the Python runner checks retained IOSurface bytes against a 4 MiB
growth budget. Run it on a macOS build worker through the build scheduler:

```sh
cargo nextest list --locked -p capture-core --message-format json > /tmp/encoder-tests.json
python3 perf/run-encoder-memory.py /tmp/encoder-tests.json 0 fixed
```

Mode `0` leaves pool ownership entirely to the production writer. Mode `1`
adds an outer pool to each test movie, a diagnostic control. The runner
creates a temporary HOME and data root. Results go under
`.lab-output/headless-pipelines/encoder/<label>/`.

## Observed on Apple silicon, macOS 27

| Variant | Footprint after 1 / 11 / 31 movies | IOSurface resident after 31 | Completion |
| --- | --- | --- | --- |
| Before, no outer pool | 16.4 / 28.6 / 50.9 MiB | 22.3 MiB (87 regions) | Failed after passing 31 movies |
| Before, outer pool | 16.6 / 20.4 / 20.9 MiB | 0 (3 regions) | All 51 |
| Writer-owned pools | 15.4 / 19.4 / 21.8 MiB | 0 (3 regions) | All 51 |

Before: roughly 1.15 MiB of footprint and 0.70 MiB of resident IOSurfaces
per completed movie over the sampled range. Writer-owned pools plateaued
at 21.8–21.9 MiB over movies 31–51, with zero resident IOSurface bytes at
every sample. These are single runs, not population estimates.

This isolates AVFoundation writer ownership. It does not exercise
ScreenCaptureKit, live capture windows, routing, or the full Annotate flow,
and does not establish the cause of a reported multi-gigabyte app footprint.
