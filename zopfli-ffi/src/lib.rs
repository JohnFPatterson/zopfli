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

fn release_prefix(ptr: *mut c_uchar) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr is a non-null malloc/realloc block the caller passed in.
    unsafe { libc::free(ptr as *mut libc::c_void) };
}

/// Append `bytes` to the malloc'd dynamic array `*out` of length `*outsize`.
///
/// A null `*out` starts empty; the incoming size is ignored in that case.
/// On failure the previous block is released and both outputs are cleared.
fn append_output(out: *mut *mut c_uchar, outsize: *mut size_t, bytes: &[u8]) -> bool {
    let len = bytes.len();
    if len == 0 {
        // SAFETY: out is a non-null pointer to the caller buffer slot.
        let old_ptr = unsafe { *out };
        release_prefix(old_ptr);
        write_failure(out, outsize);
        return false;
    }

    // SAFETY: callers pass non-null out/outsize. *out is null or a malloc block
    // whose first *outsize bytes are the prefix ZopfliCompress must keep.
    let (old_ptr, prefix_len) = unsafe {
        let old_ptr = *out;
        if old_ptr.is_null() {
            (old_ptr, 0)
        } else {
            (old_ptr, *outsize)
        }
    };

    let Some(total) = prefix_len.checked_add(len) else {
        release_prefix(old_ptr);
        write_failure(out, outsize);
        return false;
    };

    // SAFETY: old_ptr is null or a malloc/realloc block. realloc(NULL, total)
    // allocates; otherwise the first prefix_len bytes stay intact.
    let buf = unsafe { libc::realloc(old_ptr as *mut libc::c_void, total) as *mut c_uchar };
    if buf.is_null() {
        // realloc failed and left old_ptr allocated. Release it before clearing
        // the outputs so a failed append does not leak the caller's block.
        release_prefix(old_ptr);
        write_failure(out, outsize);
        return false;
    }

    // SAFETY: buf has `total` writable bytes. `bytes` has `len` bytes and does
    // not alias buf. out/outsize are non-null and writable.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf.add(prefix_len), len);
        *out = buf;
        *outsize = total;
    }
    true
}

/// Compresses according to the given output format and appends the result.
///
/// `*out` is a `malloc`'d dynamic array of `*outsize` bytes (null to start).
/// Existing bytes are kept and the compressed bytes are appended. The caller
/// must `free` `*out`.
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

    append_output(out, outsize, &compressed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish(out: *mut c_uchar, outsize: size_t) -> Vec<u8> {
        if out.is_null() {
            return Vec::new();
        }
        // SAFETY: out is a malloc block of outsize bytes produced by append_output.
        let owned = unsafe { slice::from_raw_parts(out, outsize).to_vec() };
        // SAFETY: out was allocated with malloc/realloc and is not used again.
        unsafe { libc::free(out as *mut libc::c_void) };
        owned
    }

    #[test]
    fn append_starts_from_null() {
        let mut out: *mut c_uchar = ptr::null_mut();
        let mut outsize: size_t = 0;
        assert!(append_output(&mut out, &mut outsize, &[1, 2, 3]));
        assert_eq!(finish(out, outsize), vec![1, 2, 3]);
    }

    #[test]
    fn append_keeps_existing_prefix() {
        let prefix = [9_u8, 8, 7];
        // SAFETY: malloc returns null or a writable block of prefix.len() bytes.
        let mut out = unsafe { libc::malloc(prefix.len()) as *mut c_uchar };
        assert!(!out.is_null());
        // SAFETY: out has prefix.len() writable bytes and does not alias prefix.
        unsafe { ptr::copy_nonoverlapping(prefix.as_ptr(), out, prefix.len()) };
        let mut outsize: size_t = prefix.len();

        assert!(append_output(&mut out, &mut outsize, &[1, 2, 3, 4]));
        assert_eq!(finish(out, outsize), vec![9, 8, 7, 1, 2, 3, 4]);
    }

    #[test]
    fn append_concatenates_successive_results() {
        let mut out: *mut c_uchar = ptr::null_mut();
        let mut outsize: size_t = 0;
        assert!(append_output(&mut out, &mut outsize, &[1, 2]));
        assert!(append_output(&mut out, &mut outsize, &[3, 4, 5]));
        assert_eq!(finish(out, outsize), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn null_out_ignores_stale_size() {
        let mut out: *mut c_uchar = ptr::null_mut();
        let mut outsize: size_t = 42;
        assert!(append_output(&mut out, &mut outsize, &[7, 8]));
        assert_eq!(finish(out, outsize), vec![7, 8]);
    }

    #[test]
    fn empty_append_releases_prefix() {
        // SAFETY: malloc returns null or a writable block of 4 bytes.
        let mut out = unsafe { libc::malloc(4) as *mut c_uchar };
        assert!(!out.is_null());
        let mut outsize: size_t = 4;
        assert!(!append_output(&mut out, &mut outsize, &[]));
        assert!(out.is_null());
        assert_eq!(outsize, 0);
    }
}
