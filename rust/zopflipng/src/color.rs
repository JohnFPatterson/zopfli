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

use std::collections::{HashMap, HashSet};

use png::{BitDepth, ColorType};

/// Pixel buffer and color-mode settings for the `png` crate encoder.
#[derive(Clone, Debug)]
pub(crate) struct EncodeSpec {
    pub color: ColorType,
    pub depth: BitDepth,
    pub palette: Option<Vec<u8>>,
    pub trns: Option<Vec<u8>>,
    pub pixels: Vec<u8>,
}

impl EncodeSpec {
    pub(crate) fn bytes_per_pixel(&self) -> usize {
        match self.color {
            ColorType::Grayscale | ColorType::Indexed => 1,
            ColorType::GrayscaleAlpha => 2,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
        }
    }

    /// Uncompressed filtered IDAT size (filter byte + samples per row).
    pub(crate) fn filtered_size(&self, width: u32, height: u32) -> usize {
        height as usize * (1 + width as usize * self.bytes_per_pixel())
    }
}

fn pack_rgba(px: &[u8]) -> u32 {
    px[0] as u32 + 256 * px[1] as u32 + 65536 * px[2] as u32 + 16_777_216 * px[3] as u32
}

/// Remove RGB information from pixels with alpha=0 (C++ `LossyOptimizeTransparent`).
pub(crate) fn lossy_optimize_transparent(image: &mut [u8], width: u32, height: u32) {
    let n = width as usize * height as usize;
    if image.len() < n * 4 {
        return;
    }

    let mut key = true;
    for i in 0..n {
        let a = image[i * 4 + 3];
        if a > 0 && a < 255 {
            key = false;
            break;
        }
    }

    let mut count: HashSet<u32> = HashSet::new();
    for i in 0..n {
        let mut index = pack_rgba(&image[i * 4..i * 4 + 4]);
        if image[i * 4 + 3] == 0 {
            index = 0;
        }
        count.insert(index);
        if count.len() > 256 {
            break;
        }
    }
    let palette = count.len() <= 256;

    let mut r = 0u8;
    let mut g = 0u8;
    let mut b = 0u8;
    if key || palette {
        for i in 0..n {
            if image[i * 4 + 3] == 0 {
                r = image[i * 4];
                g = image[i * 4 + 1];
                b = image[i * 4 + 2];
                break;
            }
        }
    }

    for i in 0..n {
        if image[i * 4 + 3] == 0 {
            image[i * 4] = r;
            image[i * 4 + 1] = g;
            image[i * 4 + 2] = b;
        } else if !key && !palette {
            r = image[i * 4];
            g = image[i * 4 + 1];
            b = image[i * 4 + 2];
        }
    }
}

struct Stats {
    grayscale: bool,
    opaque: bool,
    binary_alpha: bool,
    unique: Vec<[u8; 4]>,
    palettable: bool,
}

fn analyze(rgba: &[u8], width: u32, height: u32) -> Stats {
    let n = width as usize * height as usize;
    let mut grayscale = true;
    let mut opaque = true;
    let mut binary_alpha = true;
    let mut seen: HashSet<u32> = HashSet::new();
    let mut unique = Vec::new();

    for i in 0..n {
        let px = [
            rgba[i * 4],
            rgba[i * 4 + 1],
            rgba[i * 4 + 2],
            rgba[i * 4 + 3],
        ];
        if px[0] != px[1] || px[1] != px[2] {
            grayscale = false;
        }
        if px[3] != 255 {
            opaque = false;
        }
        if px[3] != 0 && px[3] != 255 {
            binary_alpha = false;
        }
        if seen.len() <= 256 {
            let packed = pack_rgba(&px);
            if seen.insert(packed) {
                unique.push(px);
            }
        }
    }

    Stats {
        grayscale,
        opaque,
        binary_alpha,
        palettable: seen.len() <= 256,
        unique,
    }
}

fn rgba_to_rgb(rgba: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n * 3];
    for i in 0..n {
        out[i * 3] = rgba[i * 4];
        out[i * 3 + 1] = rgba[i * 4 + 1];
        out[i * 3 + 2] = rgba[i * 4 + 2];
    }
    out
}

fn rgba_to_gray(rgba: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    for i in 0..n {
        out[i] = rgba[i * 4];
    }
    out
}

fn rgba_to_gray_alpha(rgba: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n * 2];
    for i in 0..n {
        out[i * 2] = rgba[i * 4];
        out[i * 2 + 1] = rgba[i * 4 + 3];
    }
    out
}

fn palette_spec(rgba: &[u8], n: usize, unique: &[[u8; 4]]) -> EncodeSpec {
    let mut plte = Vec::with_capacity(unique.len() * 3);
    let mut alphas = Vec::with_capacity(unique.len());
    let mut index_of: HashMap<u32, u8> = HashMap::with_capacity(unique.len());
    for (i, px) in unique.iter().enumerate() {
        plte.push(px[0]);
        plte.push(px[1]);
        plte.push(px[2]);
        alphas.push(px[3]);
        index_of.insert(pack_rgba(px), i as u8);
    }

    let mut pixels = vec![0u8; n];
    for i in 0..n {
        let packed = pack_rgba(&rgba[i * 4..i * 4 + 4]);
        pixels[i] = *index_of.get(&packed).unwrap_or(&0);
    }

    let mut last_trans = None;
    for (i, a) in alphas.iter().enumerate() {
        if *a != 255 {
            last_trans = Some(i);
        }
    }
    let trns = last_trans.map(|last| alphas[..=last].to_vec());

    EncodeSpec {
        color: ColorType::Indexed,
        depth: BitDepth::Eight,
        palette: Some(plte),
        trns,
        pixels,
    }
}

fn rgb_key_trns(r: u8, g: u8, b: u8) -> Vec<u8> {
    vec![0, r, 0, g, 0, b]
}

fn gray_key_trns(y: u8) -> Vec<u8> {
    vec![0, y]
}

fn color_key(rgba: &[u8], n: usize, grayscale: bool) -> Option<(u8, u8, u8)> {
    let mut key = None;
    let mut opaque: HashSet<(u8, u8, u8)> = HashSet::new();
    for i in 0..n {
        let r = rgba[i * 4];
        let g = rgba[i * 4 + 1];
        let b = rgba[i * 4 + 2];
        let a = rgba[i * 4 + 3];
        if a == 0 {
            let rgb = (r, g, b);
            if let Some(existing) = key {
                if existing != rgb {
                    return None;
                }
            } else {
                key = Some(rgb);
            }
        } else {
            opaque.insert((r, g, b));
        }
    }
    let key = key?;
    if opaque.contains(&key) {
        return None;
    }
    if grayscale && (key.0 != key.1 || key.1 != key.2) {
        return None;
    }
    Some(key)
}

fn spec_rgba(rgba: &[u8], n: usize) -> EncodeSpec {
    EncodeSpec {
        color: ColorType::Rgba,
        depth: BitDepth::Eight,
        palette: None,
        trns: None,
        pixels: rgba[..n * 4].to_vec(),
    }
}

fn spec_rgb(rgba: &[u8], n: usize) -> EncodeSpec {
    EncodeSpec {
        color: ColorType::Rgb,
        depth: BitDepth::Eight,
        palette: None,
        trns: None,
        pixels: rgba_to_rgb(rgba, n),
    }
}

/// Choose a compact color type unless `keep_colortype` forces the original.
pub(crate) fn choose_color_mode(
    rgba: &[u8],
    width: u32,
    height: u32,
    keep_colortype: bool,
    orig_color: ColorType,
) -> EncodeSpec {
    let n = width as usize * height as usize;
    let stats = analyze(rgba, width, height);

    if keep_colortype {
        return match orig_color {
            ColorType::Grayscale => EncodeSpec {
                color: ColorType::Grayscale,
                depth: BitDepth::Eight,
                palette: None,
                trns: None,
                pixels: rgba_to_gray(rgba, n),
            },
            ColorType::GrayscaleAlpha => EncodeSpec {
                color: ColorType::GrayscaleAlpha,
                depth: BitDepth::Eight,
                palette: None,
                trns: None,
                pixels: rgba_to_gray_alpha(rgba, n),
            },
            ColorType::Rgb => spec_rgb(rgba, n),
            ColorType::Rgba => spec_rgba(rgba, n),
            ColorType::Indexed => {
                if stats.palettable {
                    palette_spec(rgba, n, &stats.unique)
                } else {
                    spec_rgba(rgba, n)
                }
            }
        };
    }

    if stats.palettable && stats.unique.len() <= 256 {
        return palette_spec(rgba, n, &stats.unique);
    }
    if stats.grayscale && stats.opaque {
        return EncodeSpec {
            color: ColorType::Grayscale,
            depth: BitDepth::Eight,
            palette: None,
            trns: None,
            pixels: rgba_to_gray(rgba, n),
        };
    }
    if stats.grayscale {
        if stats.binary_alpha {
            if let Some(key) = color_key(rgba, n, true) {
                return EncodeSpec {
                    color: ColorType::Grayscale,
                    depth: BitDepth::Eight,
                    palette: None,
                    trns: Some(gray_key_trns(key.0)),
                    pixels: rgba_to_gray(rgba, n),
                };
            }
        }
        return EncodeSpec {
            color: ColorType::GrayscaleAlpha,
            depth: BitDepth::Eight,
            palette: None,
            trns: None,
            pixels: rgba_to_gray_alpha(rgba, n),
        };
    }
    if stats.opaque {
        return spec_rgb(rgba, n);
    }
    if stats.binary_alpha {
        if let Some(key) = color_key(rgba, n, false) {
            return EncodeSpec {
                color: ColorType::Rgb,
                depth: BitDepth::Eight,
                palette: None,
                trns: Some(rgb_key_trns(key.0, key.1, key.2)),
                pixels: rgba_to_rgb(rgba, n),
            };
        }
    }
    spec_rgba(rgba, n)
}

/// RGB or RGBA fallback used when a tiny paletted PNG may be larger due to PLTE.
pub(crate) fn truecolor_fallback(rgba: &[u8], width: u32, height: u32) -> EncodeSpec {
    let n = width as usize * height as usize;
    let mut opaque = true;
    for i in 0..n {
        if rgba[i * 4 + 3] != 255 {
            opaque = false;
            break;
        }
    }
    if opaque {
        spec_rgb(rgba, n)
    } else {
        spec_rgba(rgba, n)
    }
}
