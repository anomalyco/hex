# Third-Party Notices

Hex's own source is covered by the root `LICENSE`. The following notices cover
vendored icon assets and the speech models Hex downloads under their own
licences. Dependencies retain their respective upstream licenses.

## Parakeet Ultra

Parakeet Ultra by Moondream, based on NVIDIA Parakeet TDT 0.6B v3. GGUF
conversion and quantization by handy-computer. Creative Commons Attribution
4.0 International (CC BY 4.0). Hex downloads `parakeet-ultra-Q8_0.gguf` from
https://huggingface.co/handy-computer/parakeet-ultra-gguf/tree/a1539be1b849ea7463b88343e5277b373d0c4ea9
only while Labs › Parakeet Ultra is on; it does not ship the weights.

License: https://creativecommons.org/licenses/by/4.0/legalcode.en

Notice supplied with the files:

> Modifications: handy-computer converted
> https://huggingface.co/moondream/parakeet-ultra at commit 73175eb to GGUF for
> transcribe.cpp (https://github.com/handy-computer/transcribe.cpp). Conversion
> repackages the weights, renaming and re-laying-out tensors; the quantized
> files (Q8_0, Q6_K, Q5_K_M, Q4_K_M) also store them at reduced numeric
> precision. The weights were not retrained or fine-tuned.

The weights are provided as-is, without warranties, as the licence states.

## Orukeet

Orukeet by Oruk AI, based on NVIDIA Parakeet TDT 0.6B v3. GGUF conversion and
quantization by handy-computer. Creative Commons Attribution-ShareAlike 4.0
International (CC BY-SA 4.0). Hex downloads `orukeet-Q8_0.gguf` from
https://huggingface.co/handy-computer/orukeet-gguf/tree/c5511a3850fce1cbbfa5d94ef815966e1d99275f
and does not ship the weights.

License: https://creativecommons.org/licenses/by-sa/4.0/legalcode.en

Notice supplied with the files:

> # Attribution and license scope
>
> Orukeet is an adaptation of **NVIDIA Parakeet TDT 0.6B v3**. NVIDIA retains
> copyright in its model and upstream work. The base weights are distributed
> under [CC BY 4.0](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3).
> Oruk AI's changes comprise multilingual/accent continuation training,
> parameter blending, fitted and frozen Gabor-kernel replacement and recovery,
> native and ONNX export, application integration and evaluation.
> [Model stages](https://github.com/Oruk-AI/orukeet/blob/main/release/model-stages.json)
> identifies r3 as the source of every current Orukeet download and records its
> ancestry.
>
> The r3 NeMo, ONNX INT8, Q8 GGUF and F16 GGUF weights, and their fitted Gabor
> kernels, are designated **CC BY-SA 4.0**. The complete license is in
> LICENSE-WEIGHTS. This permits commercial use and modification, with
> attribution and applicable ShareAlike requirements.
> Earlier checkpoints retain their source notices and are not silently
> relicensed by this file. Orukeet v0.1.0 distributes the final r3 checkpoint in
> all four formats.
>
> Python and integration code in this repository is MIT unless a file specifies
> otherwise. The native bindings, audio windowing and worker transport derive
> from OpenWhisper by Knuckles92 and its Oruk AI integration; their MIT notice
> is retained in
> [LICENSE](https://huggingface.co/handy-computer/orukeet-gguf/blob/c5511a3850fce1cbbfa5d94ef815966e1d99275f/LICENSE).
> The downloaded NVIDIA NeMo-Speech.cpp SDK
> and its bundled dependencies retain their own notices, including Apache-2.0
> and MIT components. Keep the SDK's license files when redistributing it. The
> [pinned source](https://github.com/NVIDIA/NeMo-Speech.cpp/tree/4f9676226f667d14608487df744f375db87127f8)
> is the authority for those terms.
>
> The ONNX exporter follows sherpa-onnx's Parakeet TDT v3 conversion script. Its
> upstream reference and Apache-2.0 license are retained in
> [export/onnx](https://github.com/Oruk-AI/orukeet/blob/main/export/onnx/README.md).
> The ONNX archive includes the weight license and source attribution. The
> optimized ONNX encoder uses exactly equivalent FP32 arithmetic for 24
> quantized depthwise convolutions, with existing runtime operators and
> unchanged quantized values. Original export and separate optimization
> receipts are retained in the GitHub repository under
> `evidence/onnx-r3-20260910/` and `evidence/speed20260910/`.
>
> Training data credits:
>
> - Mozilla/Common Voice contributors: Common Voice 22, CC0.
> - Google and the FLEURS authors: FLEURS, CC BY 4.0.
> - Google and the OpenSLR 83 authors, with the `ylacombe/english_dialects`
>   restructuring: English dialect speech, CC BY-SA 4.0.
> - Beijing Kingline Data Technology and the SpeechOcean762 authors:
>   SpeechOcean762, CC BY 4.0.
> - SpeechColab and the GigaSpeechBench authors: 17 English accent/domain splits
>   in the final continuation. Their paper identifies Creative Commons source
>   audio; the source record below preserves the available license details.
> - DISCO at ETH Zurich and the EuroSpeech contributors: Bulgarian, Greek and
>   Italian parliamentary speech, with the providers' country-specific terms.
> - SberDevices and the Golos authors: Golos Crowd, under the
>   [Public license with attribution and conditions reserved](https://github.com/sberdevices/golos/blob/master/license/en_us.pdf).
> - Nordisk Språkteknologi, the National Library of Norway and the Alexandra
>   Institute: NST Swedish and Danish, distributed under CC0.
> - ILSP/Athena Research Center and the Lesbian Speech Corpus contributors:
>   dialect speech from Lesbos. The source card does not specify a reuse
>   license.
>
> Data were filtered, split, normalized and sampled as described in
> [data and licenses](https://github.com/Oruk-AI/orukeet/blob/main/docs/data-and-licenses.md).
> Corpus copyrights and source terms remain with their owners. Evaluation-only
> data have separate terms. No endorsement by NVIDIA, Mozilla, Google or the
> other source projects is implied.
>
> The generated statistical records in `evidence/metric-evidence.tar.gz`, the
> Gabor recovery count bundles under `training/gabor_half/results/`, and the
> FT-4035 benchmark counts under `evidence/regression-ft-20260907/` are
> designated CC BY 4.0, attributed to Oruk AI. This designation covers the
> prepared metric records; it does not relicense the original corpus
> recordings or transcripts, which are not included in that archive.
>
> The r3 numeric benchmark records in `evidence/standard-asr-r3-20260908/` and
> `evidence/domains-r3-20260908/` are released under CC BY 4.0. They contain
> error counts and recording identifiers; dataset audio remains with its
> providers.
>
> The paired ONNX application benchmark counts in
> `evidence/onnx-r3-20260910/` are also released under CC BY 4.0. These records
> contain numeric errors, timings and recording identifiers, without corpus
> audio or transcripts.
>
> Modifications: handy-computer converted https://huggingface.co/oruk/orukeet
> at commit b59c13a to GGUF for transcribe.cpp
> (https://github.com/handy-computer/transcribe.cpp). Conversion repackages the
> weights, renaming and re-laying-out tensors; the quantized files (Q8_0,
> Q6_K, Q5_K_M, Q4_K_M) also store them at reduced numeric precision. The weights
> were not retrained or fine-tuned.

The weights are provided as-is, without warranties, as the licence states.

## Wayland Input Method Protocol

`src/linux/protocols/input-method-unstable-v2.xml` is the upstream
`input-method-unstable-v2` protocol description, copied unchanged from
wayland-protocols-misc 0.3.12. It is MIT licensed; the copyright header is
kept in the file.

Upstream: https://gitlab.freedesktop.org/wayland/wayland-protocols

## Lucide And Feather

The settings sidebar icons in `assets/icons/` (`settings`, `modes`,
`meetings`, `history`, `voice-filter`, `scrawl`, `hud-lab`, `activity`) are
Lucide 1.62.0's `sliders-horizontal`, `layers`, `users`, `history`,
`audio-waveform`, `scan-line`, `flask-conical`, and `activity`, with stroke
attributes moved onto each element and a 1.75 stroke.
The `assets/icons/lucide/*.svg` assets in the Windows development branch are
also from Lucide: https://github.com/lucide-icons/lucide

Upstream license: https://github.com/lucide-icons/lucide/blob/23f9abc4ed0146cffededd3d7f94c1018bfdf693/LICENSE

`search.svg` has a modified handle endpoint and reordered SVG elements. The
complete upstream license below covers both original and derived assets.

ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

---

The following Lucide icons are derived from the Feather project:

airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out

The MIT License (MIT) (for the icons listed above)

Copyright (c) 2013-present Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
