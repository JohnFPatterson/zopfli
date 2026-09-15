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

//! C ABI matching `src/zopflipng/zopflipng_lib.h`.
//!
//! `CZopfliPNGOptimize` allocates the output buffer with `malloc`. Go CGO
//! (and other C callers) free it with `free`.

use std::os::raw::{c_char, c_int, c_uchar};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;

use libc::{size_t, ENOMEM};
use zopflipng::{FilterStrategy, Options};

/// Must match `CZopfliPNGOptions` in `zopflipng_lib.h` (and Go cgo).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CZopfliPNGOptions {
    pub lossy_transparent: c_int,
    pub lossy_8bit: c_int,
    pub filter_strategies: *mut c_int,
    pub num_filter_strategies: c_int,
    pub auto_filter_strategy: c_int,
    pub keepchunks: *mut *mut c_char,
    pub num_keepchunks: c_int,
    pub use_zopfli: c_int,
    pub num_iterations: c_int,
    pub num_iterations_large: c_int,
    pub block_split_strategy: c_int,
}

fn c_bool(v: c_int) -> bool {
    v != 0
}

unsafe fn options_from_c(png_options: *const CZopfliPNGOptions) -> Result<Options, c_int> {
    if png_options.is_null() {
        return Err(1);
    }
    let c = &*png_options;
    let mut opts = Options::default();
    opts.lossy_transparent = c_bool(c.lossy_transparent);
    opts.lossy_8bit = c_bool(c.lossy_8bit);
    opts.auto_filter_strategy = c_bool(c.auto_filter_strategy);
    opts.use_zopfli = c_bool(c.use_zopfli);
    opts.num_iterations = c.num_iterations;
    opts.num_iterations_large = c.num_iterations_large;
    opts.block_split_strategy = c.block_split_strategy;

    if c.num_filter_strategies > 0 {
        if c.filter_strategies.is_null() {
            return Err(1);
        }
        let n = c.num_filter_strategies as usize;
        let slice = slice::from_raw_parts(c.filter_strategies, n);
        for &v in slice {
            if let Some(s) = FilterStrategy::from_c(v) {
                opts.filter_strategies.push(s);
            }
        }
    }

    if c.num_keepchunks > 0 {
        if c.keepchunks.is_null() {
            return Err(1);
        }
        let n = c.num_keepchunks as usize;
        let ptrs = slice::from_raw_parts(c.keepchunks, n);
        for &p in ptrs {
            if p.is_null() {
                continue;
            }
            let s = std::ffi::CStr::from_ptr(p);
            opts.keepchunks
                .push(s.to_string_lossy().into_owned());
        }
    }

    Ok(opts)
}

/// Sets constructor defaults. Does not allocate `filter_strategies` or `keepchunks`.
#[no_mangle]
pub unsafe extern "C" fn CZopfliPNGSetDefaults(png_options: *mut CZopfliPNGOptions) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if png_options.is_null() {
            return;
        }
        ptr::write_bytes(png_options as *mut u8, 0, std::mem::size_of::<CZopfliPNGOptions>());
        let defaults = Options::default();
        let o = &mut *png_options;
        o.lossy_transparent = defaults.lossy_transparent as c_int;
        o.lossy_8bit = defaults.lossy_8bit as c_int;
        o.filter_strategies = ptr::null_mut();
        o.num_filter_strategies = 0;
        o.auto_filter_strategy = defaults.auto_filter_strategy as c_int;
        o.keepchunks = ptr::null_mut();
        o.num_keepchunks = 0;
        o.use_zopfli = defaults.use_zopfli as c_int;
        o.num_iterations = defaults.num_iterations;
        o.num_iterations_large = defaults.num_iterations_large;
        o.block_split_strategy = defaults.block_split_strategy;
    }));
}

/// Returns 0 on success, nonzero on error. Caller must `free(*resultpng)`.
#[no_mangle]
pub unsafe extern "C" fn CZopfliPNGOptimize(
    origpng: *const c_uchar,
    origpng_size: size_t,
    png_options: *const CZopfliPNGOptions,
    verbose: c_int,
    resultpng: *mut *mut c_uchar,
    resultpng_size: *mut size_t,
) -> c_int {
    if resultpng.is_null() || resultpng_size.is_null() {
        return 1;
    }
    *resultpng = ptr::null_mut();
    *resultpng_size = 0;

    if origpng.is_null() && origpng_size > 0 {
        return 1;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let opts = match options_from_c(png_options) {
            Ok(o) => o,
            Err(code) => return code,
        };
        let input = if origpng.is_null() {
            &[][..]
        } else {
            slice::from_raw_parts(origpng, origpng_size)
        };
        match zopflipng::optimize(input, &opts, verbose != 0) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    *resultpng_size = 0;
                    *resultpng = ptr::null_mut();
                    return 0;
                }
                let ptr = libc::malloc(bytes.len());
                if ptr.is_null() {
                    return ENOMEM;
                }
                ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
                *resultpng = ptr as *mut c_uchar;
                *resultpng_size = bytes.len();
                0
            }
            Err(_) => 1,
        }
    }));

    match result {
        Ok(code) => code,
        Err(_) => 1,
    }
}
