// Copyright 2019 Google LLC
// Copyright 2026 ZopfliPNG Rust port contributors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0

//! Behavioral stand-in for `go/zopflipng` `TestCompress`.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

fn zoidberg_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../go/zopflipng/testdata/zoidberg.png")
}

fn decode_size(png: &[u8]) -> (u32, u32) {
    let decoder = png::Decoder::new(Cursor::new(png));
    let reader = decoder.read_info().expect("valid PNG");
    let info = reader.info();
    (info.width, info.height)
}

fn decode_rgba(png: &[u8]) -> Vec<u8> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().expect("valid PNG");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("frame");
    let n = info.width as usize * info.height as usize;
    match info.color_type {
        png::ColorType::Rgba => buf[..n * 4].to_vec(),
        png::ColorType::Rgb => {
            let mut out = vec![0u8; n * 4];
            for i in 0..n {
                out[i * 4] = buf[i * 3];
                out[i * 4 + 1] = buf[i * 3 + 1];
                out[i * 4 + 2] = buf[i * 3 + 2];
                out[i * 4 + 3] = 255;
            }
            out
        }
        other => panic!("unexpected color type in test helper: {other:?}"),
    }
}

#[test]
fn compress_zoidberg_shrinks_and_stays_valid() {
    let path = zoidberg_path();
    let input = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(
        input.len() > 1000,
        "unexpected testdata size {}",
        input.len()
    );

    let output = zopflipng::optimize(&input, &zopflipng::Options::default(), false)
        .expect("optimize should succeed");

    assert!(
        output.len() < input.len(),
        "ZopfliPNG did not compress png: in={} out={}",
        input.len(),
        output.len()
    );

    let (in_w, in_h) = decode_size(&input);
    let (out_w, out_h) = decode_size(&output);
    assert_eq!((out_w, out_h), (in_w, in_h), "dimensions must match");

    let in_px = decode_rgba(&input);
    let out_px = decode_rgba(&output);
    assert_eq!(in_px, out_px, "lossless: pixels must match");
}
