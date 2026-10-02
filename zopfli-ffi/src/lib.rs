//! C ABI for libzopfli, matching `src/zopfli/zopfli.h`.

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

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

/// Output format. Discriminants match C `ZopfliFormat`.
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
    // SAFETY: caller guarantees out/outsize are valid writable pointers when non-null.
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
    // SAFETY: options is non-null and points to a writable ZopfliOptions.
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
/// Result buffer is allocated with `libc::malloc`; the caller must `free` it.
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
        // SAFETY: in_data is non-null and insize bytes are readable for this call.
        unsafe { slice::from_raw_parts(in_data, insize) }
    };

    // SAFETY: options is non-null and points to a valid ZopfliOptions for this call.
    let opts = Options::from(unsafe { *options });
    let compressed = match zopfli_core::compress(&opts, format, input) {
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

    // SAFETY: malloc returns either null or a writable block of `len` bytes.
    let buf = unsafe { libc::malloc(len) as *mut c_uchar };
    if buf.is_null() {
        write_failure(out, outsize);
        return;
    }

    // SAFETY: buf is newly allocated with `len` bytes; compressed has `len` bytes;
    // out/outsize are non-null writable pointers.
    unsafe {
        ptr::copy_nonoverlapping(compressed.as_ptr(), buf, len);
        *out = buf;
        *outsize = len;
    }
}
