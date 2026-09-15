// Copyright 2026 The Zopfli Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Core Zopfli compression.
//!
//! The public gzip/zlib/deflate wrappers still go through the crates.io `zopfli`
//! crate. The squeeze / optimal LZ77 implementation is a first-party port of
//! `src/zopfli/squeeze.c` and the helpers it needs.

use std::num::NonZeroU64;

mod cache;
mod deflate;
mod hash;
mod katajainen;
mod lz77;
mod squeeze;
mod symbols;
mod tree;
mod util;

pub use cache::LongestMatchCache;
pub use deflate::calculate_block_size;
pub use hash::ZopfliHash;
pub use lz77::{lz77_greedy, BlockState, Lz77Store};
pub use squeeze::{lz77_optimal, lz77_optimal_fixed};
pub use util::{
    ZOPFLI_CACHE_LENGTH, ZOPFLI_LARGE_FLOAT, ZOPFLI_MAX_CHAIN_HITS, ZOPFLI_MAX_MATCH,
    ZOPFLI_MIN_MATCH, ZOPFLI_NUM_D, ZOPFLI_NUM_LL, ZOPFLI_WINDOW_MASK, ZOPFLI_WINDOW_SIZE,
};

/// Options corresponding to C `ZopfliOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Whether to print output.
    pub verbose: i32,
    /// Whether to print more detailed output.
    pub verbose_more: i32,
    /// Maximum LZ77 optimization iterations (C default: 15).
    pub numiterations: i32,
    /// If non-zero, split data into multiple deflate blocks (C default: 1).
    pub blocksplitting: i32,
    /// Unused; retained for C layout compatibility (C default: 0).
    pub blocksplittinglast: i32,
    /// Maximum number of blocks to split into (C default: 15; 0 = unlimited).
    pub blocksplittingmax: i32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            verbose: 0,
            verbose_more: 0,
            numiterations: 15,
            blocksplitting: 1,
            blocksplittinglast: 0,
            blocksplittingmax: 15,
        }
    }
}

impl Options {
    /// Maps C-style options onto `zopfli::Options`.
    pub fn to_zopfli_options(self) -> zopfli::Options {
        let iterations =
            NonZeroU64::new((self.numiterations.max(1)) as u64).unwrap_or(NonZeroU64::MIN);
        zopfli::Options {
            iteration_count: iterations,
            iterations_without_improvement: iterations,
            maximum_block_splits: if self.blocksplitting != 0 {
                self.blocksplittingmax.max(0) as u16
            } else {
                1
            },
        }
    }
}

/// Output container format, matching C `ZopfliFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Gzip,
    Zlib,
    Deflate,
}

impl Format {
    fn to_zopfli_format(self) -> zopfli::Format {
        match self {
            Format::Gzip => zopfli::Format::Gzip,
            Format::Zlib => zopfli::Format::Zlib,
            Format::Deflate => zopfli::Format::Deflate,
        }
    }
}

/// Compresses `input` with Zopfli into the requested container format.
pub fn compress_bytes(
    options: Options,
    format: Format,
    input: &[u8],
) -> Result<Vec<u8>, std::io::Error> {
    let mut out = Vec::new();
    // `&[u8]` implements `Read`, including the empty slice (insize == 0).
    zopfli::compress(
        options.to_zopfli_options(),
        format.to_zopfli_format(),
        input,
        &mut out,
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn compressible_input() -> Vec<u8> {
        let mut s = String::from("compressthis");
        s.push_str(&"_foobar".repeat(1000));
        s.push('$');
        s.into_bytes()
    }

    #[test]
    fn gzip_compressible_under_500_and_roundtrips() {
        let input = compressible_input();
        let compressed = compress_bytes(Options::default(), Format::Gzip, &input)
            .expect("gzip compression should succeed");
        assert!(
            compressed.len() <= 500,
            "compressed size {} exceeds 500 bytes",
            compressed.len()
        );

        let mut decoder = flate2::read::GzDecoder::new(compressed.as_slice());
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .expect("gzip should decode");
        assert_eq!(decompressed, input);
    }

    #[test]
    fn gzip_empty_input_is_valid() {
        let compressed = compress_bytes(Options::default(), Format::Gzip, &[])
            .expect("empty gzip compression should succeed");
        assert!(
            compressed.len() <= 20,
            "empty gzip size {} exceeds 20 bytes",
            compressed.len()
        );

        let mut decoder = flate2::read::GzDecoder::new(compressed.as_slice());
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .expect("empty gzip should decode");
        assert!(decompressed.is_empty());
    }

    #[test]
    fn default_options_match_c() {
        let opts = Options::default();
        assert_eq!(opts.verbose, 0);
        assert_eq!(opts.verbose_more, 0);
        assert_eq!(opts.numiterations, 15);
        assert_eq!(opts.blocksplitting, 1);
        assert_eq!(opts.blocksplittinglast, 0);
        assert_eq!(opts.blocksplittingmax, 15);
    }
}
