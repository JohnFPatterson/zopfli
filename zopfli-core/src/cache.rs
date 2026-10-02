//! Longest-match cache from `src/zopfli/cache.c`.
//!
//! `ZOPFLI_LONGEST_MATCH_CACHE` is enabled and `ZOPFLI_CACHE_LENGTH` is 8.

use crate::consts::CACHE_LENGTH;

/// Cache of previously found length/distance pairs and sub-lengths.
pub struct LongestMatchCache {
    pub length: Vec<u16>,
    pub dist: Vec<u16>,
    pub sublen: Vec<u8>,
}

impl LongestMatchCache {
    /// Allocate and initialize, matching `ZopfliInitCache`.
    ///
    /// `length` is 1, `dist` is 0, and `sublen` is 0. Length 1 with distance 0
    /// is the sentinel for an unfilled entry.
    pub fn new(blocksize: usize) -> Self {
        Self {
            length: vec![1; blocksize],
            dist: vec![0; blocksize],
            sublen: vec![0; CACHE_LENGTH * blocksize * 3],
        }
    }

    /// Store a sublen array, matching `ZopfliSublenToCache`.
    pub fn sublen_to_cache(&mut self, sublen: &[u16], pos: usize, length: usize) {
        let mut j: usize = 0;
        let mut bestlength: u32 = 0;
        let cache_off = CACHE_LENGTH * pos * 3;
        if length < 3 {
            return;
        }
        let mut i: usize = 3;
        while i <= length {
            if i == length || sublen[i] != sublen[i + 1] {
                self.sublen[cache_off + j * 3] = (i - 3) as u8;
                self.sublen[cache_off + j * 3 + 1] = (sublen[i] % 256) as u8;
                self.sublen[cache_off + j * 3 + 2] = ((sublen[i] >> 8) % 256) as u8;
                bestlength = i as u32;
                j += 1;
                if j >= CACHE_LENGTH {
                    break;
                }
            }
            i += 1;
        }
        if j < CACHE_LENGTH {
            debug_assert!(bestlength as usize == length);
            self.sublen[cache_off + (CACHE_LENGTH - 1) * 3] = bestlength.wrapping_sub(3) as u8;
        } else {
            debug_assert!(bestlength as usize <= length);
        }
        debug_assert!(bestlength == self.max_cached_sublen(pos, length));
    }

    /// Expand a cached sublen array, matching `ZopfliCacheToSublen`.
    pub fn cache_to_sublen(&self, pos: usize, length: usize, sublen: &mut [u16]) {
        let maxlength = self.max_cached_sublen(pos, length);
        let mut prevlength: u32 = 0;
        if length < 3 {
            return;
        }
        let cache_off = CACHE_LENGTH * pos * 3;
        for j in 0..CACHE_LENGTH {
            // Shadows the `length` parameter, as in the C function.
            let length = u32::from(self.sublen[cache_off + j * 3]) + 3;
            let dist = u32::from(self.sublen[cache_off + j * 3 + 1])
                + 256 * u32::from(self.sublen[cache_off + j * 3 + 2]);
            let mut i = prevlength as usize;
            while i <= length as usize {
                sublen[i] = dist as u16;
                i += 1;
            }
            if length == maxlength {
                break;
            }
            prevlength = length + 1;
        }
    }

    /// Length up to which sub-lengths could be stored, matching `ZopfliMaxCachedSublen`.
    pub fn max_cached_sublen(&self, pos: usize, length: usize) -> u32 {
        let _ = length;
        let cache_off = CACHE_LENGTH * pos * 3;
        if self.sublen[cache_off + 1] == 0 && self.sublen[cache_off + 2] == 0 {
            return 0;
        }
        u32::from(self.sublen[cache_off + (CACHE_LENGTH - 1) * 3]) + 3
    }
}
