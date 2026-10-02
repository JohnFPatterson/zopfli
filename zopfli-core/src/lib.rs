//! Safe Rust port of libzopfli (`src/zopfli`).
//!
//! All compression logic lives here. The C ABI is in `zopfli-ffi`.

#![forbid(unsafe_code)]

mod blocksplitter;
mod cache;
mod consts;
mod container;
mod deflate;
mod hash;
mod katajainen;
mod lz77;
mod squeeze;
mod symbols;
mod tree;

use std::fmt;

/// Error from compression or option validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Invalid or unsupported format discriminant.
    InvalidFormat,
    /// Allocation or encoding failure.
    CompressFailed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidFormat => write!(f, "invalid format"),
            Error::CompressFailed => write!(f, "compression failed"),
        }
    }
}

impl std::error::Error for Error {}

/// Options corresponding to C `ZopfliOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub verbose: i32,
    pub verbose_more: i32,
    pub numiterations: i32,
    pub blocksplitting: i32,
    pub blocksplittinglast: i32,
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

/// Output container format, matching C `ZopfliFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Gzip,
    Zlib,
    Deflate,
}

/// Compress `input` into `format` using Zopfli.
///
/// Matches `ZopfliCompress`: gzip and zlib wrap a dynamic final DEFLATE block.
pub fn compress(options: &Options, format: Format, input: &[u8]) -> Result<Vec<u8>, Error> {
    let out = match format {
        Format::Gzip => container::gzip_compress(options, input),
        Format::Zlib => container::zlib_compress(options, input),
        Format::Deflate => container::deflate_compress(options, input),
    };
    if out.is_empty() {
        Err(Error::CompressFailed)
    } else {
        Ok(out)
    }
}
