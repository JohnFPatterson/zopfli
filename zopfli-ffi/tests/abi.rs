//! Pointer-level ABI smoke tests for `zopfli-ffi`.

use std::ptr;
use zopfli_ffi::{ZopfliCompress, ZopfliFormat, ZopfliInitOptions, ZopfliOptions};

#[test]
fn init_options_null_is_noop() {
    // SAFETY: null is explicitly allowed by the contract.
    unsafe { ZopfliInitOptions(ptr::null_mut()) };
}

#[test]
fn init_options_writes_defaults() {
    let mut opts = ZopfliOptions {
        verbose: 1,
        verbose_more: 1,
        numiterations: 0,
        blocksplitting: 0,
        blocksplittinglast: 1,
        blocksplittingmax: 0,
    };
    // SAFETY: opts is a valid writable ZopfliOptions.
    unsafe { ZopfliInitOptions(&mut opts) };
    assert_eq!(opts.numiterations, 15);
    assert_eq!(opts.blocksplitting, 1);
    assert_eq!(opts.blocksplittingmax, 15);
}

#[test]
fn compress_empty_gzip_mallocs_buffer() {
    let opts = ZopfliOptions {
        verbose: 0,
        verbose_more: 0,
        numiterations: 1,
        blocksplitting: 1,
        blocksplittinglast: 0,
        blocksplittingmax: 15,
    };
    let mut out: *mut u8 = ptr::null_mut();
    let mut outsize: usize = 0;
    // SAFETY: pointers are valid for this call; empty input allows null in_data.
    unsafe {
        ZopfliCompress(
            &opts,
            ZopfliFormat::Gzip,
            ptr::null(),
            0,
            &mut out,
            &mut outsize,
        );
    }
    assert!(!out.is_null());
    assert_eq!(outsize, 20);
    // SAFETY: buffer was allocated with malloc by ZopfliCompress.
    unsafe { libc::free(out.cast()) };
}

#[test]
fn compress_null_out_is_noop() {
    let opts = ZopfliOptions {
        verbose: 0,
        verbose_more: 0,
        numiterations: 1,
        blocksplitting: 1,
        blocksplittinglast: 0,
        blocksplittingmax: 15,
    };
    let mut outsize = 99usize;
    // SAFETY: out is null — function must return without writing.
    unsafe {
        ZopfliCompress(
            &opts,
            ZopfliFormat::Gzip,
            ptr::null(),
            0,
            ptr::null_mut(),
            &mut outsize,
        );
    }
    assert_eq!(outsize, 99);
}
