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

use std::io::{Cursor, Read, Write};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use png::{BitDepth, ColorType};

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

fn color_type_code(color: ColorType) -> u8 {
    match color {
        ColorType::Grayscale => 0,
        ColorType::Rgb => 2,
        ColorType::Indexed => 3,
        ColorType::GrayscaleAlpha => 4,
        ColorType::Rgba => 6,
    }
}

fn bit_depth_code(depth: BitDepth) -> u8 {
    match depth {
        BitDepth::One => 1,
        BitDepth::Two => 2,
        BitDepth::Four => 4,
        BitDepth::Eight => 8,
        BitDepth::Sixteen => 16,
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let ia = a as i16;
    let ib = b as i16;
    let ic = c as i16;
    let p = ia + ib - ic;
    let pa = (p - ia).abs();
    let pb = (p - ib).abs();
    let pc = (p - ic).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn filter_row(filter: u8, bpp: usize, prev: &[u8], curr: &[u8], out: &mut [u8]) {
    out[0] = filter;
    for i in 0..curr.len() {
        let x = curr[i];
        let a = if i >= bpp { curr[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        out[i + 1] = match filter {
            1 => x.wrapping_sub(a),
            2 => x.wrapping_sub(b),
            3 => x.wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
            4 => x.wrapping_sub(paeth(a, b, c)),
            _ => x,
        };
    }
}

fn abs_sum(filtered: &[u8]) -> u64 {
    filtered
        .iter()
        .map(|b| i8::from_le_bytes([*b]).unsigned_abs() as u64)
        .sum()
}

fn shannon_score(filtered: &[u8]) -> u64 {
    let mut counts = [0u32; 256];
    for &b in filtered {
        counts[b as usize] += 1;
    }
    let n = filtered.len() as f64;
    let mut ent = 0.0f64;
    for c in counts {
        if c > 0 {
            let p = f64::from(c) / n;
            ent -= p * p.log2();
        }
    }
    (ent * 1_000_000.0) as u64
}

fn pick_row_filter(bpp: usize, prev: &[u8], curr: &[u8], entropy: bool) -> u8 {
    let mut best = 0u8;
    let mut best_score = u64::MAX;
    let mut tmp = vec![0u8; curr.len() + 1];
    for f in 0..5u8 {
        filter_row(f, bpp, prev, curr, &mut tmp);
        let score = if entropy {
            shannon_score(&tmp[1..])
        } else {
            abs_sum(&tmp[1..])
        };
        if score < best_score {
            best_score = score;
            best = f;
        }
    }
    best
}

fn filter_image(
    pixels: &[u8],
    width: usize,
    height: usize,
    bpp: usize,
    strategy: FilterStrategy,
    orig_filters: Option<&[u8]>,
) -> Vec<u8> {
    let stride = width * bpp;
    let mut out = vec![0u8; height * (1 + stride)];
    let zeros = vec![0u8; stride];
    for y in 0..height {
        let curr = &pixels[y * stride..(y + 1) * stride];
        let prev = if y == 0 {
            zeros.as_slice()
        } else {
            &pixels[(y - 1) * stride..y * stride]
        };
        let dest = &mut out[y * (1 + stride)..(y + 1) * (1 + stride)];
        let filter = match strategy {
            FilterStrategy::Zero => 0,
            FilterStrategy::One => 1,
            FilterStrategy::Two => 2,
            FilterStrategy::Three => 3,
            FilterStrategy::Four => 4,
            FilterStrategy::MinSum | FilterStrategy::BruteForce => {
                pick_row_filter(bpp, prev, curr, false)
            }
            FilterStrategy::Entropy => pick_row_filter(bpp, prev, curr, true),
            FilterStrategy::Predefined => orig_filters
                .and_then(|f| f.get(y).copied())
                .filter(|f| *f <= 4)
                .unwrap_or(0),
        };
        filter_row(filter, bpp, prev, curr, dest);
    }
    out
}

fn flate2_zlib(data: &[u8], level: u32) -> Result<Vec<u8>, Error> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level));
    encoder
        .write_all(data)
        .map_err(|e| Error::Encode(e.to_string()))?;
    encoder.finish().map_err(|e| Error::Encode(e.to_string()))
}

fn zopfli_zlib(data: &[u8], iterations: u64) -> Result<Vec<u8>, Error> {
    let opts = zopfli_core::Options {
        numiterations: iterations.max(1) as i32,
        ..zopfli_core::Options::default()
    };
    zopfli_core::compress_bytes(opts, zopfli_core::Format::Zlib, data)
        .map_err(|e| Error::Encode(format!("zopfli: {e}")))
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

fn assemble_png(spec: &EncodeSpec, width: u32, height: u32, idat: &[u8]) -> Vec<u8> {
    let mut out = PNG_SIG.to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(bit_depth_code(spec.depth));
    ihdr.push(color_type_code(spec.color));
    ihdr.extend_from_slice(&[0, 0, 0]); // compression, filter, interlace
    write_chunk(&mut out, b"IHDR", &ihdr);
    if let Some(ref pal) = spec.palette {
        write_chunk(&mut out, b"PLTE", pal);
    }
    if let Some(ref trns) = spec.trns {
        write_chunk(&mut out, b"tRNS", trns);
    }
    write_chunk(&mut out, b"IDAT", idat);
    write_chunk(&mut out, b"IEND", &[]);
    out
}

fn filtered_scanlines(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    strategy: FilterStrategy,
    orig_filters: Option<&[u8]>,
) -> Vec<u8> {
    filter_image(
        &spec.pixels,
        width as usize,
        height as usize,
        spec.bytes_per_pixel(),
        strategy,
        orig_filters,
    )
}

/// Cheap zlib (level 1) of filtered scanlines — analogue of C++ window=8192, no Zopfli.
pub(crate) fn cheap_encode(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    strategy: FilterStrategy,
    orig_filters: Option<&[u8]>,
) -> Result<Vec<u8>, Error> {
    let filtered = filtered_scanlines(spec, width, height, strategy, orig_filters);
    let idat = flate2_zlib(&filtered, 1)?;
    Ok(assemble_png(spec, width, height, &idat))
}

pub(crate) fn best_encode(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    strategy: FilterStrategy,
    orig_filters: Option<&[u8]>,
) -> Result<Vec<u8>, Error> {
    let filtered = filtered_scanlines(spec, width, height, strategy, orig_filters);
    let idat = flate2_zlib(&filtered, 9)?;
    Ok(assemble_png(spec, width, height, &idat))
}

/// Filter scanlines then compress the IDAT zlib payload with Zopfli (32 KiB window).
pub(crate) fn encode_zopfli(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    strategy: FilterStrategy,
    orig_filters: Option<&[u8]>,
    iterations: u64,
) -> Result<Vec<u8>, Error> {
    let filtered = filtered_scanlines(spec, width, height, strategy, orig_filters);
    let idat = zopfli_zlib(&filtered, iterations)?;
    Ok(assemble_png(spec, width, height, &idat))
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

fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| Error::Encode(format!("inflate IDAT: {e}")))?;
    Ok(out)
}

/// Per-row filter bytes from the original file (`kStrategyPredefined`).
pub(crate) fn extract_filter_bytes(png: &[u8]) -> Result<Vec<u8>, Error> {
    let (width, height, bit_depth, color) = ihdr_meta(png)?;
    if bit_depth != 8 {
        return Err(Error::Decode("predefined filters need 8-bit input".into()));
    }
    let bpp = match color {
        ColorType::Grayscale | ColorType::Indexed => 1,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
    };
    let stride = 1 + width as usize * bpp;
    let raw = inflate_zlib(&concat_idat(png)?)?;
    if raw.len() != stride * height as usize {
        return Err(Error::Decode("unexpected IDAT size for filters".into()));
    }
    Ok((0..height as usize).map(|y| raw[y * stride]).collect())
}

pub(crate) fn for_each_chunk(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zopfli_zlib_shrinks_zeros() {
        let data = vec![0u8; 10_000];
        let out = zopfli_zlib(&data, 5).unwrap();
        assert!(out.len() < 64, "got {} bytes", out.len());
    }
}
