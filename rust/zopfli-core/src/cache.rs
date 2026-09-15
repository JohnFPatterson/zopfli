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

//! Longest-match cache (`src/zopfli/cache.c`).

use crate::util::ZOPFLI_CACHE_LENGTH;

/// Cache used by `ZopfliFindLongestMatch` to remember previously found
/// length/dist values.
pub struct LongestMatchCache {
    pub(crate) length: Vec<u16>,
    pub(crate) dist: Vec<u16>,
    pub(crate) sublen: Vec<u8>,
}

impl LongestMatchCache {
    /// Initializes the cache for a block of `blocksize` bytes.
    pub fn new(blocksize: usize) -> Self {
        Self {
            // length > 0 and dist 0 is invalid, meaning this slot is empty.
            length: vec![1; blocksize],
            dist: vec![0; blocksize],
            sublen: vec![0; ZOPFLI_CACHE_LENGTH * 3 * blocksize],
        }
    }

    /// Stores `sublen` in the cache, matching `ZopfliSublenToCache`.
    pub fn sublen_to_cache(&mut self, sublen: &[u16], pos: usize, length: usize) {
        if length < 3 {
            return;
        }
        let cache = ZOPFLI_CACHE_LENGTH * pos * 3;
        let mut j = 0usize;
        let mut bestlength = 0usize;
        for i in 3..=length {
            if i == length || sublen[i] != sublen[i + 1] {
                self.sublen[cache + j * 3] = (i - 3) as u8;
                self.sublen[cache + j * 3 + 1] = (sublen[i] % 256) as u8;
                self.sublen[cache + j * 3 + 2] = ((sublen[i] >> 8) % 256) as u8;
                bestlength = i;
                j += 1;
                if j >= ZOPFLI_CACHE_LENGTH {
                    break;
                }
            }
        }
        if j < ZOPFLI_CACHE_LENGTH {
            debug_assert_eq!(bestlength, length);
            self.sublen[cache + (ZOPFLI_CACHE_LENGTH - 1) * 3] = (bestlength - 3) as u8;
        } else {
            debug_assert!(bestlength <= length);
        }
        debug_assert_eq!(bestlength, self.max_cached_sublen(pos) as usize);
    }

    /// Extracts `sublen` from the cache, matching `ZopfliCacheToSublen`.
    pub fn cache_to_sublen(&self, pos: usize, length: usize, sublen: &mut [u16]) {
        if length < 3 {
            return;
        }
        let maxlength = self.max_cached_sublen(pos) as usize;
        let mut prevlength = 0usize;
        let cache = ZOPFLI_CACHE_LENGTH * pos * 3;
        for j in 0..ZOPFLI_CACHE_LENGTH {
            let cached_length = usize::from(self.sublen[cache + j * 3]) + 3;
            let dist = u16::from(self.sublen[cache + j * 3 + 1])
                + 256 * u16::from(self.sublen[cache + j * 3 + 2]);
            for item in sublen.iter_mut().take(cached_length + 1).skip(prevlength) {
                *item = dist;
            }
            if cached_length == maxlength {
                break;
            }
            prevlength = cached_length + 1;
        }
    }

    /// Returns the length up to which could be stored in the cache.
    pub fn max_cached_sublen(&self, pos: usize) -> u32 {
        let cache = ZOPFLI_CACHE_LENGTH * pos * 3;
        if self.sublen[cache + 1] == 0 && self.sublen[cache + 2] == 0 {
            return 0;
        }
        u32::from(self.sublen[cache + (ZOPFLI_CACHE_LENGTH - 1) * 3]) + 3
    }
}
