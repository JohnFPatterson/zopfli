//! C ABI for libzopfli, matching `src/zopfli/zopfli.h`.

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

use libc::{c_int, c_uchar, size_t};
use std::ptr;
use std::slice;
use zopfli_core::{Format, ZopfliOptions as CoreOptions};

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

impl From<ZopfliOptions> for CoreOptions {
    fn from(opts: ZopfliOptions) -> Self {
        CoreOptions {
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

/// Initializes options with default values (same as C `ZopfliInitOptions`).
///
/// # Safety
/// `options` must be null or point to a valid writable `ZopfliOptions`.
#[no_mangle]
pub unsafe extern "C" fn ZopfliInitOptions(options: *mut ZopfliOptions) {
    if options.is_null() {
        return;
    }
    // SAFETY: caller promised `options` is writable for one `ZopfliOptions`.
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
///
/// # Safety
/// - `options` must be non-null and readable for the duration of the call.
/// - `out` and `outsize` must be non-null and writable.
/// - If `insize != 0`, `in_data` must point to `insize` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn ZopfliCompress(
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
        // SAFETY: out/outsize checked non-null above.
        unsafe {
            *out = ptr::null_mut();
            *outsize = 0;
        }
        return;
    }

    let Some(format) = map_format(output_type as c_int) else {
        // SAFETY: out/outsize checked non-null above.
        unsafe {
            *out = ptr::null_mut();
            *outsize = 0;
        }
        return;
    };

    let input: &[u8] = if insize == 0 || in_data.is_null() {
        &[]
    } else {
        // SAFETY: caller promised `in_data` points to `insize` readable bytes.
        unsafe { slice::from_raw_parts(in_data, insize) }
    };

    // SAFETY: caller promised `options` is readable.
    let opts = CoreOptions::from(unsafe { *options });
    let compressed = match zopfli_core::compress(&opts, format, input) {
        Ok(bytes) => bytes,
        Err(_) => {
            // SAFETY: out/outsize checked non-null above.
            unsafe {
                *out = ptr::null_mut();
                *outsize = 0;
            }
            return;
        }
    };

    let len = compressed.len();
    if len == 0 {
        // SAFETY: out/outsize checked non-null above.
        unsafe {
            *out = ptr::null_mut();
            *outsize = 0;
        }
        return;
    }

    // SAFETY: malloc returns null or a block of at least `len` bytes.
    let buf = unsafe { libc::malloc(len) as *mut c_uchar };
    if buf.is_null() {
        // SAFETY: out/outsize checked non-null above.
        unsafe {
            *out = ptr::null_mut();
            *outsize = 0;
        }
        return;
    }

    // SAFETY: `buf` has `len` writable bytes; `compressed` has `len` bytes;
    // `out`/`outsize` are non-null writable pointers.
    unsafe {
        ptr::copy_nonoverlapping(compressed.as_ptr(), buf, len);
        *out = buf;
        *outsize = len;
    }
}
