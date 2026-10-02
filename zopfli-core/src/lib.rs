//! Safe Rust port of libzopfli (`src/zopfli`).
//!
//! Algorithm sources are adapted from the Apache-2.0
//! [`zopfli`](https://github.com/zopfli-rs/zopfli) crate (v0.8.3), which
//! reimplements Google's C Zopfli. Large hash tables use heap `Vec`s so this
//! crate can `forbid(unsafe_code)`. Behavior is checked against the in-tree C
//! oracle via differential drivers.

#![forbid(unsafe_code)]
#![deny(trivial_casts, trivial_numeric_casts)]

#[macro_use]
extern crate alloc;

pub use deflate::{BlockType, DeflateEncoder};
pub use gzip::GzipEncoder;
pub use zlib::ZlibEncoder;

// Vendored encoder modules expect these std I/O names at the crate root.
pub use std::io::{Error, Write};

mod blocksplitter;
mod cache;
mod deflate;
mod gzip;
mod hash;
mod iter;
mod katajainen;
mod lz77;
mod squeeze;
mod symbols;
mod tree;
mod util;
mod zlib;

use core::fmt;
use core::num::NonZeroU64;

/// Algorithm options used by the encoder internals (maps from C `ZopfliOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Options {
    /// Maximum LZ77 optimization iterations (C `numiterations`).
    pub iteration_count: NonZeroU64,
    /// Stop after this many iterations without improvement.
    pub iterations_without_improvement: NonZeroU64,
    /// Maximum block splits (C `blocksplittingmax`; 0 = unlimited).
    pub maximum_block_splits: u16,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            iteration_count: NonZeroU64::new(15).unwrap_or(NonZeroU64::MIN),
            iterations_without_improvement: NonZeroU64::new(u64::MAX).unwrap_or(NonZeroU64::MIN),
            maximum_block_splits: 15,
        }
    }
}

/// Options corresponding to C `ZopfliOptions` field layout / defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZopfliOptions {
    pub verbose: i32,
    pub verbose_more: i32,
    pub numiterations: i32,
    pub blocksplitting: i32,
    pub blocksplittinglast: i32,
    pub blocksplittingmax: i32,
}

impl Default for ZopfliOptions {
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

impl ZopfliOptions {
    /// Map C-style options onto algorithm options.
    pub fn to_algo_options(self) -> Options {
        let iters = NonZeroU64::new(self.numiterations.max(1) as u64).unwrap_or(NonZeroU64::MIN);
        Options {
            iteration_count: iters,
            // C runs a fixed iteration budget; do not early-stop for "no improvement".
            iterations_without_improvement: NonZeroU64::new(u64::MAX).unwrap_or(NonZeroU64::MIN),
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

/// Public compression error (malformed options / encode failure).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompressError {
    /// Invalid or unsupported format discriminant.
    InvalidFormat,
    /// Encoding failure.
    CompressFailed,
    /// I/O failure while writing compressed output.
    Io,
}

impl fmt::Display for CompressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompressError::InvalidFormat => write!(f, "invalid format"),
            CompressError::CompressFailed => write!(f, "compression failed"),
            CompressError::Io => write!(f, "I/O error"),
        }
    }
}

impl std::error::Error for CompressError {}

/// Compress `input` into `format` using Zopfli (C `ZopfliCompress` behavior).
pub fn compress(
    options: &ZopfliOptions,
    format: Format,
    input: &[u8],
) -> Result<Vec<u8>, CompressError> {
    let algo = options.to_algo_options();
    let mut out = Vec::new();
    match format {
        Format::Gzip => {
            let mut enc = GzipEncoder::new_buffered(algo, BlockType::Dynamic, &mut out)
                .map_err(|_| CompressError::CompressFailed)?;
            std::io::copy(&mut &*input, &mut enc).map_err(|_| CompressError::Io)?;
            enc.into_inner()
                .map_err(|_| CompressError::Io)?
                .finish()
                .map_err(|_| CompressError::CompressFailed)?;
        }
        Format::Zlib => {
            let mut enc = ZlibEncoder::new_buffered(algo, BlockType::Dynamic, &mut out)
                .map_err(|_| CompressError::CompressFailed)?;
            std::io::copy(&mut &*input, &mut enc).map_err(|_| CompressError::Io)?;
            enc.into_inner()
                .map_err(|_| CompressError::Io)?
                .finish()
                .map_err(|_| CompressError::CompressFailed)?;
        }
        Format::Deflate => {
            let mut enc = DeflateEncoder::new_buffered(algo, BlockType::Dynamic, &mut out);
            std::io::copy(&mut &*input, &mut enc).map_err(|_| CompressError::Io)?;
            enc.into_inner()
                .map_err(|_| CompressError::Io)?
                .finish()
                .map_err(|_| CompressError::CompressFailed)?;
        }
    }
    Ok(out)
}
