// Copyright 2013 Google Inc. All Rights Reserved.
// Copyright 2026 ZopfliPNG Rust port contributors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.

use std::io::{Cursor, Read};
use std::num::NonZeroU64;

use flate2::read::ZlibDecoder;
use png::{AdaptiveFilterType, BitDepth, ColorType, FilterType};

use crate::color::EncodeSpec;
use crate::{Error, FilterStrategy};

pub(crate) const PNG_SIG: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10];

pub(crate) struct DecodedPng {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub src_color: ColorType,
}

pub(crate) fn ihdr_meta(png: &[u8]) -> Result<(u32, u32, u8, ColorType), Error> {
    if png.len() < 33 || &png[0..8] != PNG_SIG {
        return Err(Error::Decode("not a PNG file".into()));
    }
    let len = u32::from_be_bytes(png[8..12].try_into().unwrap());
    if len != 13 || &png[12..16] != b"IHDR" {
        return Err(Error::Decode("missing IHDR".into()));
    }
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    let bit_depth = png[24];
    let color = match png[25] {
        0 => ColorType::Grayscale,
        2 => ColorType::Rgb,
        3 => ColorType::Indexed,
        4 => ColorType::GrayscaleAlpha,
        6 => ColorType::Rgba,
        _ => return Err(Error::Decode("invalid IHDR color type".into())),
    };
    Ok((width, height, bit_depth, color))
}

pub(crate) fn decode_rgba(data: &[u8]) -> Result<DecodedPng, Error> {
    let (_, _, _src_bit_depth, src_color) = ihdr_meta(data)?;
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| Error::Decode(e.to_string()))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| Error::Decode(e.to_string()))?;
    let width = info.width;
    let height = info.height;
    let rgba = expand_to_rgba(&buf, width, height, info.color_type, info.bit_depth)?;
    Ok(DecodedPng {
        width,
        height,
        rgba,
        src_color,
    })
}

fn expand_to_rgba(
    buf: &[u8],
    width: u32,
    height: u32,
    color: ColorType,
    depth: BitDepth,
) -> Result<Vec<u8>, Error> {
    if depth != BitDepth::Eight {
        return Err(Error::Decode("expected 8-bit samples after expand".into()));
    }
    let n = width as usize * height as usize;
    match color {
        ColorType::Rgba => {
            if buf.len() < n * 4 {
                return Err(Error::Decode("truncated RGBA frame".into()));
            }
            Ok(buf[..n * 4].to_vec())
        }
        ColorType::Rgb => {
            if buf.len() < n * 3 {
                return Err(Error::Decode("truncated RGB frame".into()));
            }
            let mut out = vec![0u8; n * 4];
            for i in 0..n {
                out[i * 4] = buf[i * 3];
                out[i * 4 + 1] = buf[i * 3 + 1];
                out[i * 4 + 2] = buf[i * 3 + 2];
                out[i * 4 + 3] = 255;
            }
            Ok(out)
        }
        ColorType::Grayscale => {
            if buf.len() < n {
                return Err(Error::Decode("truncated gray frame".into()));
            }
            let mut out = vec![0u8; n * 4];
            for i in 0..n {
                let g = buf[i];
                out[i * 4] = g;
                out[i * 4 + 1] = g;
                out[i * 4 + 2] = g;
                out[i * 4 + 3] = 255;
            }
            Ok(out)
        }
        ColorType::GrayscaleAlpha => {
            if buf.len() < n * 2 {
                return Err(Error::Decode("truncated gray+alpha frame".into()));
            }
            let mut out = vec![0u8; n * 4];
            for i in 0..n {
                let g = buf[i * 2];
                let a = buf[i * 2 + 1];
                out[i * 4] = g;
                out[i * 4 + 1] = g;
                out[i * 4 + 2] = g;
                out[i * 4 + 3] = a;
            }
            Ok(out)
        }
        ColorType::Indexed => Err(Error::Decode(
            "palette was not expanded by the png crate".into(),
        )),
    }
}

#[derive(Clone, Copy)]
enum FilterChoice {
    Fixed(FilterType),
    Adaptive,
}

fn map_strategy(strategy: FilterStrategy) -> FilterChoice {
    match strategy {
        FilterStrategy::Zero => FilterChoice::Fixed(FilterType::NoFilter),
        FilterStrategy::One => FilterChoice::Fixed(FilterType::Sub),
        FilterStrategy::Two => FilterChoice::Fixed(FilterType::Up),
        FilterStrategy::Three => FilterChoice::Fixed(FilterType::Avg),
        FilterStrategy::Four => FilterChoice::Fixed(FilterType::Paeth),
        FilterStrategy::MinSum
        | FilterStrategy::Entropy
        | FilterStrategy::Predefined
        | FilterStrategy::BruteForce => FilterChoice::Adaptive,
    }
}

pub(crate) fn encode_png(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    strategy: FilterStrategy,
    compression: png::Compression,
) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(spec.color);
        encoder.set_depth(spec.depth);
        encoder.set_compression(compression);
        match map_strategy(strategy) {
            FilterChoice::Fixed(filter) => {
                encoder.set_adaptive_filter(AdaptiveFilterType::NonAdaptive);
                encoder.set_filter(filter);
            }
            FilterChoice::Adaptive => {
                encoder.set_adaptive_filter(AdaptiveFilterType::Adaptive);
            }
        }
        if let Some(ref pal) = spec.palette {
            encoder.set_palette(pal.clone());
        }
        if let Some(ref trns) = spec.trns {
            encoder.set_trns(trns.clone());
        }
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::Encode(e.to_string()))?;
        writer
            .write_image_data(&spec.pixels)
            .map_err(|e| Error::Encode(e.to_string()))?;
        writer.finish().map_err(|e| Error::Encode(e.to_string()))?;
    }
    Ok(buf)
}

fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::Encode(format!("inflate IDAT: {e}")))?;
    Ok(out)
}

fn zopfli_zlib(data: &[u8], iterations: u64) -> Result<Vec<u8>, Error> {
    let mut opts = zopfli::Options::default();
    opts.iteration_count = NonZeroU64::new(iterations.max(1)).expect("iterations >= 1");
    let mut out = Vec::new();
    zopfli::compress(opts, zopfli::Format::Zlib, data, &mut out)
        .map_err(|e| Error::Encode(format!("zopfli: {e}")))?;
    Ok(out)
}

fn write_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(ty);
    hasher.update(data);
    out.extend_from_slice(&hasher.finalize().to_be_bytes());
}

fn concat_idat(png: &[u8]) -> Result<Vec<u8>, Error> {
    let mut idat = Vec::new();
    for_each_chunk(png, |ty, data, _raw| {
        if ty == b"IDAT" {
            idat.extend_from_slice(data);
        }
        Ok(())
    })?;
    if idat.is_empty() {
        return Err(Error::Encode("PNG has no IDAT".into()));
    }
    Ok(idat)
}

/// Recompress the filtered IDAT payload with Zopfli zlib (window size 32768).
pub(crate) fn recompress_idat_zopfli(png: &[u8], iterations: u64) -> Result<Vec<u8>, Error> {
    let idat = concat_idat(png)?;
    let filtered = inflate_zlib(&idat)?;
    let compressed = zopfli_zlib(&filtered, iterations)?;
    replace_idat(png, &compressed)
}

fn replace_idat(png: &[u8], new_idat: &[u8]) -> Result<Vec<u8>, Error> {
    if png.len() < 8 || &png[0..8] != PNG_SIG {
        return Err(Error::Encode("not a PNG file".into()));
    }
    let mut out = png[0..8].to_vec();
    let mut written_idat = false;
    for_each_chunk(png, |ty, _data, raw| {
        if ty == b"IDAT" {
            if !written_idat {
                write_chunk(&mut out, b"IDAT", new_idat);
                written_idat = true;
            }
        } else {
            out.extend_from_slice(raw);
        }
        Ok(())
    })?;
    if !written_idat {
        return Err(Error::Encode("PNG has no IDAT".into()));
    }
    Ok(out)
}

fn for_each_chunk(
    png: &[u8],
    mut f: impl FnMut(&[u8; 4], &[u8], &[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    if png.len() < 8 || &png[0..8] != PNG_SIG {
        return Err(Error::Decode("not a PNG file".into()));
    }
    let mut i = 8usize;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        if i + 12 + len > png.len() {
            return Err(Error::Decode("truncated PNG chunk".into()));
        }
        let ty: [u8; 4] = png[i + 4..i + 8].try_into().unwrap();
        let data = &png[i + 8..i + 8 + len];
        let raw = &png[i..i + 12 + len];
        f(&ty, data, raw)?;
        i += 12 + len;
        if &ty == b"IEND" {
            break;
        }
    }
    Ok(())
}

/// Copy named ancillary chunks from `orig` into `png`, preserving IHDR/PLTE/IDAT locations.
pub(crate) fn keep_chunks(
    orig: &[u8],
    keepnames: &[String],
    png: &mut Vec<u8>,
) -> Result<(), Error> {
    if keepnames.is_empty() {
        return Ok(());
    }
    let mut loc0: Vec<Vec<u8>> = Vec::new();
    let mut loc1: Vec<Vec<u8>> = Vec::new();
    let mut loc2: Vec<Vec<u8>> = Vec::new();
    let mut loc = 0u8;
    for_each_chunk(orig, |ty, _data, raw| {
        let name = std::str::from_utf8(ty).unwrap_or("");
        if ty == b"PLTE" {
            loc = 1;
        }
        if ty == b"IDAT" {
            loc = 2;
        }
        if keepnames.iter().any(|k| k == name) && ty != b"IHDR" && ty != b"IDAT" && ty != b"IEND" {
            let owned = raw.to_vec();
            match loc {
                0 => loc0.push(owned),
                1 => loc1.push(owned),
                _ => loc2.push(owned),
            }
        }
        Ok(())
    })?;

    if loc0.is_empty() && loc1.is_empty() && loc2.is_empty() {
        return Ok(());
    }

    let mut out = png[0..8].to_vec();
    let mut inserted_pre_idat = false;
    for_each_chunk(png, |ty, _data, raw| {
        if ty == b"IDAT" && !inserted_pre_idat {
            for c in &loc0 {
                out.extend_from_slice(c);
            }
            for c in &loc1 {
                out.extend_from_slice(c);
            }
            inserted_pre_idat = true;
        }
        if ty == b"IEND" {
            for c in &loc2 {
                out.extend_from_slice(c);
            }
        }
        out.extend_from_slice(raw);
        Ok(())
    })?;
    *png = out;
    Ok(())
}

/// Cheap deflate pass used to pick a filter strategy (C++ uses window size 8192, no Zopfli).
pub(crate) fn cheap_encode(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    strategy: FilterStrategy,
) -> Result<Vec<u8>, Error> {
    encode_png(spec, width, height, strategy, png::Compression::Fast)
}

pub(crate) fn best_encode(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    strategy: FilterStrategy,
) -> Result<Vec<u8>, Error> {
    encode_png(spec, width, height, strategy, png::Compression::Best)
}
