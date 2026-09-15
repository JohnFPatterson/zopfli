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

//! C ABI for libzopfli, matching `src/zopfli/zopfli.h`.
//!
//! Output buffers are allocated with `libc::malloc` so Go CGO can free them
//! with `C.free`.

use libc::{c_int, c_uchar, size_t};
use std::ptr;
use std::slice;
use zopfli_core::{Format, Options};

/// Options used throughout the program. Field order matches C `ZopfliOptions`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ZopfliOptions {
    pub verbose: c_int,
    pub verbose_more: c_int,
    pub numiterations: c_int,
    pub blocksplitting: c_int,
    pub blocksplittinglast: c_int,
    pub blocksplittingmax: c_int,
}

/// Output format. Discriminants match C `ZopfliFormat` (passed as `int`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZopfliFormat {
    Gzip = 0,
    Zlib = 1,
    Deflate = 2,
}

impl From<ZopfliOptions> for Options {
    fn from(opts: ZopfliOptions) -> Self {
        Options {
            verbose: opts.verbose,
            verbose_more: opts.verbose_more,
            numiterations: opts.numiterations,
            blocksplitting: opts.blocksplitting,
            blocksplittinglast: opts.blocksplittinglast,
            blocksplittingmax: opts.blocksplittingmax,
        }
    }
}

fn map_format(output_type: c_int) -> Option<Format> {
    match output_type {
        0 => Some(Format::Gzip),
        1 => Some(Format::Zlib),
        2 => Some(Format::Deflate),
        _ => None,
    }
}

fn write_failure(out: *mut *mut c_uchar, outsize: *mut size_t) {
    unsafe {
        *out = ptr::null_mut();
        *outsize = 0;
    }
}

/// Initializes options with default values (same as C `ZopfliInitOptions`).
#[no_mangle]
pub extern "C" fn ZopfliInitOptions(options: *mut ZopfliOptions) {
    if options.is_null() {
        return;
    }
    unsafe {
        *options = ZopfliOptions {
            verbose: 0,
            verbose_more: 0,
            numiterations: 15,
            blocksplitting: 1,
            blocksplittinglast: 0,
            blocksplittingmax: 15,
        };
    }
}

/// Compresses according to the given output format.
///
/// The result is written to a `malloc`'d buffer; the caller must `free` it.
/// Null required pointers return without crashing.
#[no_mangle]
pub extern "C" fn ZopfliCompress(
    options: *const ZopfliOptions,
    output_type: ZopfliFormat,
    in_data: *const c_uchar,
    insize: size_t,
    out: *mut *mut c_uchar,
    outsize: *mut size_t,
) {
    if options.is_null() || out.is_null() || outsize.is_null() {
        return;
    }

    if in_data.is_null() && insize != 0 {
        write_failure(out, outsize);
        return;
    }

    let Some(format) = map_format(output_type as c_int) else {
        write_failure(out, outsize);
        return;
    };

    let input: &[u8] = if insize == 0 || in_data.is_null() {
        &[]
    } else {
        unsafe { slice::from_raw_parts(in_data, insize) }
    };

    let opts = Options::from(unsafe { *options });
    let compressed = match zopfli_core::compress_bytes(opts, format, input) {
        Ok(bytes) => bytes,
        Err(_) => {
            write_failure(out, outsize);
            return;
        }
    };

    let len = compressed.len();
    if len == 0 {
        write_failure(out, outsize);
        return;
    }

    let buf = unsafe { libc::malloc(len) as *mut c_uchar };
    if buf.is_null() {
        write_failure(out, outsize);
        return;
    }

    unsafe {
        ptr::copy_nonoverlapping(compressed.as_ptr(), buf, len);
        *out = buf;
        *outsize = len;
    }
}
