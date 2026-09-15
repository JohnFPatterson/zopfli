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

//! Constants matching `src/zopfli/util.h`.

/// Minimum length that can be encoded in deflate.
pub const ZOPFLI_MIN_MATCH: usize = 3;
/// Maximum length that can be encoded in deflate.
pub const ZOPFLI_MAX_MATCH: usize = 258;

/// Number of distinct literal/length symbols in DEFLATE.
pub const ZOPFLI_NUM_LL: usize = 288;
/// Number of distinct distance symbols in DEFLATE.
pub const ZOPFLI_NUM_D: usize = 32;

/// The window size for deflate. Must be a power of two.
pub const ZOPFLI_WINDOW_SIZE: usize = 32768;
/// The window mask used to wrap indices into the window.
pub const ZOPFLI_WINDOW_MASK: usize = ZOPFLI_WINDOW_SIZE - 1;

/// Used to initialize costs.
pub const ZOPFLI_LARGE_FLOAT: f64 = 1e30;

/// Longest-match cache slots per input byte.
pub const ZOPFLI_CACHE_LENGTH: usize = 8;

/// Limit the max hash chain hits for this hash value.
pub const ZOPFLI_MAX_CHAIN_HITS: usize = 8192;
