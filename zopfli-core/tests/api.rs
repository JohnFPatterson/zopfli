//! Round-trip and option defaults for the public core API.
//! There is no in-tree C unit suite; Go CGO tests exercise the shared library.

use zopfli_core::{compress, Format, ZopfliOptions};

#[test]
fn init_defaults_match_c() {
    let o = ZopfliOptions::default();
    assert_eq!(o.verbose, 0);
    assert_eq!(o.verbose_more, 0);
    assert_eq!(o.numiterations, 15);
    assert_eq!(o.blocksplitting, 1);
    assert_eq!(o.blocksplittinglast, 0);
    assert_eq!(o.blocksplittingmax, 15);
}

#[test]
fn gzip_empty_has_header_and_trailer() {
    let o = ZopfliOptions {
        numiterations: 1,
        ..ZopfliOptions::default()
    };
    let out = compress(&o, Format::Gzip, &[]).expect("empty gzip");
    assert_eq!(out.len(), 20);
    assert_eq!(&out[0..3], &[0x1f, 0x8b, 0x08]);
}

#[test]
fn gzip_hello_matches_known_c_bytes() {
    let o = ZopfliOptions {
        numiterations: 1,
        ..ZopfliOptions::default()
    };
    let out = compress(&o, Format::Gzip, b"hello zopfli").expect("gzip");
    let expected = hex_decode("1f8b0800000000000203cb48cdc9c957a8ca2f48cbc9040044df6aa80c000000");
    assert_eq!(out, expected);
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}
