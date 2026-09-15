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
//
// Rust port of src/zopflipng/zopflipng_lib.cc. Decode/encode uses the `png`
// crate; IDAT zlib payloads are produced with the crates.io `zopfli` crate.

//! PNG optimizer using Zopfli deflate compression.
//!
//! This is a behavioral port of Google ZopfliPNG: decode a PNG, strip
//! ancillary chunks by default, pick a compact color type and a scanline
//! filter strategy, then recompress the IDAT with Zopfli.

mod color;
mod png_io;

use std::fmt;

use color::{choose_color_mode, lossy_optimize_transparent, truecolor_fallback, EncodeSpec};
use png::ColorType;
use png_io::{
    best_encode, cheap_encode, decode_rgba, encode_zopfli, extract_filter_bytes, keep_chunks,
    DecodedPng,
};

/// PNG filter strategies from `zopflipng_lib.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FilterStrategy {
    Zero = 0,
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    MinSum = 5,
    Entropy = 6,
    Predefined = 7,
    BruteForce = 8,
}

impl FilterStrategy {
    /// Convert a C `enum ZopfliPNGFilterStrategy` value.
    pub fn from_c(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::MinSum,
            6 => Self::Entropy,
            7 => Self::Predefined,
            8 => Self::BruteForce,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::One => "one",
            Self::Two => "two",
            Self::Three => "three",
            Self::Four => "four",
            Self::MinSum => "minimum sum",
            Self::Entropy => "entropy",
            Self::Predefined => "predefined",
            Self::BruteForce => "brute force",
        }
    }
}

/// Optimization options matching `ZopfliPNGOptions` defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Alter hidden colors of fully transparent pixels.
    pub lossy_transparent: bool,
    /// Convert 16-bit per channel images to 8-bit (always true in this port).
    pub lossy_8bit: bool,
    /// Filter strategies to try when `auto_filter_strategy` is false.
    pub filter_strategies: Vec<FilterStrategy>,
    /// Cheap-compress all common filters, then Zopfli only the winner.
    pub auto_filter_strategy: bool,
    /// Keep the original PNG color type instead of auto-converting.
    pub keep_colortype: bool,
    /// Ancillary chunk names (e.g. `"gAMA"`) to copy from the input PNG.
    pub keepchunks: Vec<String>,
    /// Use Zopfli for the final IDAT zlib payload.
    pub use_zopfli: bool,
    /// Zopfli iterations when uncompressed IDAT input is < 200_000 bytes.
    pub num_iterations: i32,
    /// Zopfli iterations for larger images.
    pub num_iterations_large: i32,
    /// Unused; kept for ABI / C++ compatibility.
    pub block_split_strategy: i32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            lossy_transparent: false,
            lossy_8bit: false,
            filter_strategies: Vec::new(),
            auto_filter_strategy: true,
            keep_colortype: false,
            keepchunks: Vec::new(),
            use_zopfli: true,
            num_iterations: 15,
            num_iterations_large: 5,
            block_split_strategy: 1,
        }
    }
}

/// Optimization failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Decode(String),
    Encode(String),
    InvalidInput(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Decode(s) => write!(f, "decoding error: {s}"),
            Error::Encode(s) => write!(f, "encoding error: {s}"),
            Error::InvalidInput(s) => write!(f, "invalid input: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// Strategies used for the cheap auto-filter pass (C++ skips brute force).
fn auto_filter_candidates() -> Vec<FilterStrategy> {
    vec![
        FilterStrategy::Zero,
        FilterStrategy::One,
        FilterStrategy::Two,
        FilterStrategy::Three,
        FilterStrategy::Four,
        FilterStrategy::MinSum,
        FilterStrategy::Entropy,
        FilterStrategy::Predefined,
    ]
}

fn enabled_strategies(options: &Options) -> Vec<FilterStrategy> {
    if options.auto_filter_strategy {
        auto_filter_candidates()
    } else if !options.filter_strategies.is_empty() {
        options.filter_strategies.clone()
    } else {
        // C++ would encode nothing; Zero is a safe fallback for the C ABI.
        vec![FilterStrategy::Zero]
    }
}

fn auto_choose_filter(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    candidates: &[FilterStrategy],
    orig_filters: Option<&[u8]>,
    verbose: bool,
) -> Result<FilterStrategy, Error> {
    let mut best: Option<(FilterStrategy, usize)> = None;
    for &strategy in candidates {
        let encoded = cheap_encode(spec, width, height, strategy, orig_filters)?;
        if verbose {
            println!(
                "Filter strategy {} (fast): {} bytes",
                strategy.name(),
                encoded.len()
            );
        }
        if best.map_or(true, |(_, size)| encoded.len() < size) {
            best = Some((strategy, encoded.len()));
        }
    }
    best.map(|(s, _)| s)
        .ok_or_else(|| Error::Encode("no filter strategy produced output".into()))
}

fn try_optimize(
    spec: &EncodeSpec,
    width: u32,
    height: u32,
    strategy: FilterStrategy,
    orig_filters: Option<&[u8]>,
    use_zopfli: bool,
    iterations: u64,
) -> Result<Vec<u8>, Error> {
    if use_zopfli {
        encode_zopfli(spec, width, height, strategy, orig_filters, iterations)
    } else {
        best_encode(spec, width, height, strategy, orig_filters)
    }
}

/// Recompress and optimize a PNG.
///
/// Returns a new PNG on success. Ancillary chunks are stripped unless
/// `options.keepchunks` lists them. The IDAT zlib payload is produced with
/// Zopfli when `options.use_zopfli` is true.
pub fn optimize(origpng: &[u8], options: &Options, verbose: bool) -> Result<Vec<u8>, Error> {
    if origpng.is_empty() {
        return Err(Error::Decode("empty input".into()));
    }

    let DecodedPng {
        width,
        height,
        mut rgba,
        src_color,
    } = decode_rgba(origpng)?;

    let mut keep_colortype = options.keep_colortype;
    if options
        .keepchunks
        .iter()
        .any(|c| c == "bKGD" || c == "sBIT")
    {
        if !keep_colortype && verbose {
            println!("Forced to keep original color type due to keeping bKGD or sBIT chunk.");
        }
        keep_colortype = true;
    }

    // 16-bit is always reduced to 8-bit in this port (`lossy_8bit` is accepted
    // but 16-bit keep is not implemented).
    let _ = options.lossy_8bit;

    if options.lossy_transparent {
        lossy_optimize_transparent(&mut rgba, width, height);
    }

    let spec = choose_color_mode(&rgba, width, height, keep_colortype, src_color);
    let orig_filters = extract_filter_bytes(origpng).ok();
    let orig_filters = orig_filters.as_deref();
    let mut strategies = enabled_strategies(options);

    if options.auto_filter_strategy {
        let winner = auto_choose_filter(&spec, width, height, &strategies, orig_filters, verbose)?;
        strategies = vec![winner];
    }

    let raw_len = spec.filtered_size(width, height);
    let iterations = if raw_len < 200_000 {
        options.num_iterations
    } else {
        options.num_iterations_large
    };
    let iterations = iterations.max(1) as u64;

    let mut best: Option<Vec<u8>> = None;
    for strategy in strategies {
        let out = try_optimize(
            &spec,
            width,
            height,
            strategy,
            orig_filters,
            options.use_zopfli,
            iterations,
        )?;
        if verbose {
            println!("Filter strategy {}: {} bytes", strategy.name(), out.len());
        }
        if best.as_ref().map_or(true, |b| out.len() < b.len()) {
            best = Some(out);
        }
    }

    // C++: for very small paletted output, also try truecolor (no PLTE overhead).
    if !keep_colortype
        && spec.color == ColorType::Indexed
        && best.as_ref().map_or(false, |b| b.len() < 4096)
    {
        let fallback = truecolor_fallback(&rgba, width, height);
        let strategy = FilterStrategy::MinSum;
        if let Ok(out) = try_optimize(
            &fallback,
            width,
            height,
            strategy,
            orig_filters,
            options.use_zopfli,
            iterations,
        ) {
            if best.as_ref().map_or(true, |b| out.len() < b.len()) {
                best = Some(out);
            }
        }
    }

    let mut result = best.ok_or_else(|| Error::Encode("no encoding produced output".into()))?;

    if !options.keepchunks.is_empty() {
        keep_chunks(origpng, &options.keepchunks, &mut result)?;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_c() {
        let o = Options::default();
        assert!(!o.lossy_transparent);
        assert!(!o.lossy_8bit);
        assert!(o.auto_filter_strategy);
        assert!(!o.keep_colortype);
        assert!(o.use_zopfli);
        assert_eq!(o.num_iterations, 15);
        assert_eq!(o.num_iterations_large, 5);
        assert_eq!(o.block_split_strategy, 1);
        assert!(o.filter_strategies.is_empty());
        assert!(o.keepchunks.is_empty());
    }

    #[test]
    fn reject_invalid_png() {
        let err = optimize(b"not a png", &Options::default(), false).unwrap_err();
        match err {
            Error::Decode(_) => {}
            other => panic!("expected decode error, got {other:?}"),
        }
    }
}
