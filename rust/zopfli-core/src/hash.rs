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

//! Hash for `ZopfliFindLongestMatch` (`src/zopfli/hash.c`).

use crate::util::{ZOPFLI_MIN_MATCH, ZOPFLI_WINDOW_MASK, ZOPFLI_WINDOW_SIZE};

const HASH_SHIFT: i32 = 5;
const HASH_MASK: i32 = 32767;

/// Sliding-window hash used by longest-match search.
///
/// `ZOPFLI_HASH_SAME` and `ZOPFLI_HASH_SAME_HASH` are always enabled, matching
/// the C build.
pub struct ZopfliHash {
    pub(crate) head: Vec<i32>,
    pub(crate) prev: Vec<u16>,
    pub(crate) hashval: Vec<i32>,
    pub(crate) val: i32,
    pub(crate) head2: Vec<i32>,
    pub(crate) prev2: Vec<u16>,
    pub(crate) hashval2: Vec<i32>,
    pub(crate) val2: i32,
    pub(crate) same: Vec<u16>,
}

impl ZopfliHash {
    /// Allocates hash tables for `ZOPFLI_WINDOW_SIZE`, matching `ZopfliAllocHash`.
    pub fn new() -> Self {
        Self::with_window_size(ZOPFLI_WINDOW_SIZE)
    }

    /// Allocates hash tables for the given window size.
    pub fn with_window_size(window_size: usize) -> Self {
        let mut h = Self {
            head: vec![0; 65536],
            prev: vec![0; window_size],
            hashval: vec![0; window_size],
            val: 0,
            head2: vec![0; 65536],
            prev2: vec![0; window_size],
            hashval2: vec![0; window_size],
            val2: 0,
            same: vec![0; window_size],
        };
        h.reset(window_size);
        h
    }

    /// Resets all fields, matching `ZopfliResetHash`.
    pub fn reset(&mut self, window_size: usize) {
        self.val = 0;
        self.head.fill(-1);
        for i in 0..window_size {
            self.prev[i] = i as u16;
            self.hashval[i] = -1;
        }
        self.same[..window_size].fill(0);

        self.val2 = 0;
        self.head2.fill(-1);
        for i in 0..window_size {
            self.prev2[i] = i as u16;
            self.hashval2[i] = -1;
        }
    }

    fn update_hash_value(&mut self, c: u8) {
        self.val = ((self.val << HASH_SHIFT) ^ i32::from(c)) & HASH_MASK;
    }

    /// Updates hash values at `pos`. All calls must be for consecutive bytes.
    pub fn update(&mut self, array: &[u8], pos: usize, end: usize) {
        let hpos = pos & ZOPFLI_WINDOW_MASK;
        let c = if pos + ZOPFLI_MIN_MATCH <= end {
            array[pos + ZOPFLI_MIN_MATCH - 1]
        } else {
            0
        };
        self.update_hash_value(c);
        self.hashval[hpos] = self.val;
        if self.head[self.val as usize] != -1
            && self.hashval[self.head[self.val as usize] as usize] == self.val
        {
            self.prev[hpos] = self.head[self.val as usize] as u16;
        } else {
            self.prev[hpos] = hpos as u16;
        }
        self.head[self.val as usize] = hpos as i32;

        let mut amount: usize = 0;
        if self.same[pos.wrapping_sub(1) & ZOPFLI_WINDOW_MASK] > 1 {
            amount = usize::from(self.same[pos.wrapping_sub(1) & ZOPFLI_WINDOW_MASK] - 1);
        }
        while pos + amount + 1 < end
            && array[pos] == array[pos + amount + 1]
            && amount < usize::from(u16::MAX)
        {
            amount += 1;
        }
        self.same[hpos] = amount as u16;

        // Match C: `(unsigned short - int) & 255`, which uses signed promotion.
        self.val2 = ((i32::from(self.same[hpos]) - ZOPFLI_MIN_MATCH as i32) & 255) ^ self.val;
        self.hashval2[hpos] = self.val2;
        if self.head2[self.val2 as usize] != -1
            && self.hashval2[self.head2[self.val2 as usize] as usize] == self.val2
        {
            self.prev2[hpos] = self.head2[self.val2 as usize] as u16;
        } else {
            self.prev2[hpos] = hpos as u16;
        }
        self.head2[self.val2 as usize] = hpos as i32;
    }

    /// Prepopulates the hash before `update` can be used correctly.
    pub fn warmup(&mut self, array: &[u8], pos: usize, end: usize) {
        self.update_hash_value(array[pos]);
        if pos + 1 < end {
            self.update_hash_value(array[pos + 1]);
        }
    }
}

impl Default for ZopfliHash {
    fn default() -> Self {
        Self::new()
    }
}
