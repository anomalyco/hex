use super::*;

/// An SPS written by libx264 for 64x48 High profile video (it contains an emulation prevention
/// byte).
const X264_SPS: &str = "6764000aacd9447b0110000003001000000303c0f1225960";

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
        .collect()
}

fn bit_string(bytes: &[u8], bits: usize) -> String {
    let mut reader = BitReader::new(bytes);
    (0..bits)
        .map(|_| if reader.bit().expect("bit") { '1' } else { '0' })
        .collect()
}

#[test]
fn writes_exp_golomb_codes() {
    let mut bits = BitWriter::default();
    for value in [0, 1, 2, 3, 7] {
        bits.ue(value);
    }
    bits.se(1);
    bits.se(-1);
    bits.se(0);
    let bytes = bits.finish();
    assert_eq!(bytes.len(), 4);
    assert_eq!(
        bit_string(&bytes, 32),
        [
            "1", "010", "011", "00100", "0001000", "010", "011", "1", "1", "00000"
        ]
        .concat()
    );
}

#[test]
fn exp_golomb_codes_round_trip() {
    let unsigned = [0, 1, 2, 30, 255, 256, 65_535, 1 << 20, u32::MAX - 1];
    let signed = [0, 1, -1, 2, -2, 1000, -1000, i32::MAX, -i32::MAX];
    let mut bits = BitWriter::default();
    for value in unsigned {
        bits.ue(value);
    }
    for value in signed {
        bits.se(value);
    }
    bits.bits(0b101, 3);
    let bytes = bits.finish();
    let mut reader = BitReader::new(&bytes);
    for value in unsigned {
        assert_eq!(reader.ue().expect("ue"), value);
    }
    for value in signed {
        assert_eq!(reader.se().expect("se"), value);
    }
    assert_eq!(reader.bits(3).expect("bits"), 0b101);
    assert!(reader.bit().expect("stop bit"));
}

#[test]
fn reader_refuses_truncated_and_oversized_codes() {
    assert!(BitReader::new(&[0x00]).ue().is_err());
    assert!(BitReader::new(&[0, 0, 0, 0, 0x80]).ue().is_err());
}

#[test]
fn emulation_prevention_round_trips() {
    let cases: [(&[u8], &[u8]); 6] = [
        (&[0, 0, 0], &[0, 0, 3, 0]),
        (&[0, 0, 1], &[0, 0, 3, 1]),
        (&[0, 0, 2, 0, 0, 3], &[0, 0, 3, 2, 0, 0, 3, 3]),
        (&[0, 0, 4], &[0, 0, 4]),
        (&[0, 0, 0, 0, 0], &[0, 0, 3, 0, 0, 3, 0]),
        (&[1, 0, 2, 0, 0], &[1, 0, 2, 0, 0]),
    ];
    for (rbsp, payload) in cases {
        assert_eq!(escape(rbsp), payload, "escape {rbsp:?}");
        assert_eq!(unescape(payload), rbsp, "unescape {payload:?}");
    }
}

#[test]
fn parses_a_high_profile_sps() {
    assert_eq!(
        parse_sps(&hex(X264_SPS)).expect("parse SPS"),
        Sps {
            id: 0,
            chroma_array_type: 1,
            log2_max_frame_num: 4,
            log2_max_poc_lsb: 6,
            max_num_ref_frames: 4,
            pic_size_in_mbs: 12,
        }
    );
}

#[test]
fn skip_pps_and_slice_parse_back() {
    let sps = parse_sps(&hex(X264_SPS)).expect("parse SPS");
    let pps_nal = skip_pps(1, 0);
    assert_eq!(pps_nal[0], 0x68);
    let pps = parse_pps(&pps_nal).expect("parse PPS");
    assert_eq!((pps.id, pps.sps_id), (1, 0));
    let parameters = ParameterSets {
        sps: vec![sps.clone()],
        pps: vec![pps],
    };
    let picture = SkipPicture {
        frame_num: 9,
        poc_lsb: 42,
        abs_diff_pic_num_minus1: 2,
    };
    let slice = skip_slice(&sps, 1, picture);
    assert_eq!(slice[0], 0x01, "non-reference coded slice");
    let header = parse_slice_header(&slice, &parameters).expect("parse slice");
    assert_eq!(
        header,
        SliceHeader {
            idr: false,
            pps_id: 1,
            frame_num: 9,
            poc_lsb: 42,
            delta_poc_bottom: 0,
            marking: Marking::NonReference,
        }
    );

    let rbsp = unescape(&slice[1..]);
    let mut reader = BitReader::new(&rbsp);
    assert_eq!(reader.ue().expect("first_mb_in_slice"), 0);
    assert_eq!(reader.ue().expect("slice_type"), 5);
    assert_eq!(reader.ue().expect("pps id"), 1);
    assert_eq!(reader.bits(4).expect("frame_num"), 9);
    assert_eq!(reader.bits(6).expect("poc lsb"), 42);
    assert!(!reader.bit().expect("override flag"));
    assert!(reader.bit().expect("modification flag"));
    assert_eq!(reader.ue().expect("idc"), 0);
    assert_eq!(reader.ue().expect("abs_diff_pic_num_minus1"), 2);
    assert_eq!(reader.ue().expect("end of modifications"), 3);
    assert_eq!(reader.se().expect("slice_qp_delta"), 0);
    assert_eq!(reader.ue().expect("disable_deblocking_filter_idc"), 1);
    assert_eq!(reader.ue().expect("mb_skip_run"), 12);
    assert!(reader.bit().expect("stop bit"));
    while let Ok(bit) = reader.bit() {
        assert!(!bit, "alignment zero bits");
    }
}

#[test]
fn finds_the_first_slice_after_other_nal_units() {
    let sample = [0, 0, 0, 2, 0x06, 0xff, 0, 0, 0, 2, 0x41, 0x9a];
    assert_eq!(first_slice(&sample, 4).expect("slice"), &[0x41, 0x9a]);
    assert!(first_slice(&sample[..11], 4).is_err());
    assert!(first_slice(&[0, 2, 0x06, 0xff], 2).is_err());
}

fn sps(max_num_ref_frames: u32) -> Sps {
    Sps {
        id: 0,
        chroma_array_type: 1,
        log2_max_frame_num: 4,
        log2_max_poc_lsb: 4,
        max_num_ref_frames,
        pic_size_in_mbs: 12,
    }
}

fn picture(frame_num: u32, poc_lsb: u32, marking: Marking) -> SliceHeader {
    SliceHeader {
        idr: matches!(marking, Marking::Idr { .. }),
        pps_id: 0,
        frame_num,
        poc_lsb,
        delta_poc_bottom: 0,
        marking,
    }
}

/// I0 P8 Bref4 b2 b6 in decode order, with poc lsb wrapping at 16.
fn pyramid() -> Vec<SliceHeader> {
    vec![
        picture(0, 0, Marking::Idr { long_term: false }),
        picture(1, 8, Marking::SlidingWindow),
        picture(2, 4, Marking::SlidingWindow),
        picture(3, 2, Marking::NonReference),
        picture(3, 6, Marking::NonReference),
    ]
}

#[test]
fn plans_a_picture_after_the_final_frame() {
    assert_eq!(
        plan_skip_picture(&pyramid(), 1, &sps(4)).expect("plan"),
        SkipPicture {
            frame_num: 3,
            poc_lsb: 10,
            // CurrPicNum 3 minus the P picture's number 1.
            abs_diff_pic_num_minus1: 1,
        }
    );
}

#[test]
fn wraps_frame_numbers_and_picture_order_counts() {
    let mut pictures = vec![picture(0, 0, Marking::Idr { long_term: false })];
    // References at POC 2, 4, ... 28 (lsb wraps at 16), frame_num wraps at 16.
    for index in 1..=14_u32 {
        pictures.push(picture(
            index % 16,
            (index * 2) % 16,
            Marking::SlidingWindow,
        ));
    }
    pictures.push(picture(15, 30 % 16, Marking::SlidingWindow));
    pictures.push(picture(0, 26 % 16, Marking::NonReference));
    let last = pictures.len() - 2;
    let skip = plan_skip_picture(&pictures, last, &sps(4)).expect("plan");
    assert_eq!(skip.frame_num, 0, "frame_num wraps");
    assert_eq!(skip.poc_lsb, 32 % 16, "picture order count wraps");
    // CurrPicNum 0; the final frame's FrameNumWrap is 15 - 16 = -1.
    assert_eq!(skip.abs_diff_pic_num_minus1, 0);
}

#[test]
fn applies_unmarking_operations() {
    let mut pictures = pyramid();
    // The final B unmarks the P picture (picNumX = 2 - (0 + 1) = 1).
    pictures[2].marking = Marking::Adaptive(vec![Some(0)]);
    assert!(matches!(
        plan_skip_picture(&pictures, 1, &sps(4)),
        Err(FinalFrameHoldError::Unsupported(
            "final frame is not a reference picture"
        ))
    ));
    pictures[2].marking = Marking::Adaptive(vec![None]);
    assert!(matches!(
        plan_skip_picture(&pictures, 1, &sps(4)),
        Err(FinalFrameHoldError::Unsupported(
            "memory management operation other than unmarking"
        ))
    ));
}

#[test]
fn sliding_window_can_evict_the_final_frame() {
    assert!(matches!(
        plan_skip_picture(&pyramid(), 1, &sps(1)),
        Err(FinalFrameHoldError::Unsupported(
            "final frame is not a reference picture"
        ))
    ));
    assert!(plan_skip_picture(&pyramid(), 1, &sps(2)).is_ok());
}

#[test]
fn refuses_unsupported_streams() {
    let mut pictures = pyramid();
    pictures[1].marking = Marking::NonReference;
    pictures[2].frame_num = 1;
    pictures[3].frame_num = 2;
    pictures[4].frame_num = 2;
    assert!(matches!(
        plan_skip_picture(&pictures, 1, &sps(4)),
        Err(FinalFrameHoldError::Unsupported(
            "final frame is not a reference picture"
        ))
    ));
    assert!(matches!(
        plan_skip_picture(&pyramid(), 2, &sps(4)),
        Err(FinalFrameHoldError::Unsupported(
            "presentation order disagrees with picture order count"
        ))
    ));
    assert!(matches!(
        plan_skip_picture(&pyramid()[1..], 0, &sps(4)),
        Err(FinalFrameHoldError::Unsupported(_))
    ));
    let mut gap = pyramid();
    gap[2].frame_num = 3;
    assert!(matches!(
        plan_skip_picture(&gap, 1, &sps(4)),
        Err(FinalFrameHoldError::Unsupported(
            "frame numbers out of sequence"
        ))
    ));
}
