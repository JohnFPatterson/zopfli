//! Sliding hash from `src/zopfli/hash.c`.
//!
//! `ZOPFLI_HASH_SAME` and `ZOPFLI_HASH_SAME_HASH` are both enabled.

use crate::consts::{MIN_MATCH, WINDOW_MASK};

const HASH_SHIFT: i32 = 5;
const HASH_MASK: i32 = 32767;

/// Hash chains used by longest-match search.
///
/// `head` / `head2` have length 65536. `prev`, `hashval`, `same`, `prev2`,
/// and `hashval2` have length `window_size`.
pub struct Hash {
    pub head: Vec<i32>,
    pub prev: Vec<u16>,
    pub hashval: Vec<i32>,
    pub val: i32,
    pub head2: Vec<i32>,
    pub prev2: Vec<u16>,
    pub hashval2: Vec<i32>,
    pub val2: i32,
    pub same: Vec<u16>,
}

impl Hash {
    /// Allocate and reset, matching `ZopfliAllocHash` followed by `ZopfliResetHash`.
    pub fn new(window_size: usize) -> Self {
        let mut hash = Self {
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
        hash.reset(window_size);
        hash
    }

    /// Reset every field, matching `ZopfliResetHash`.
    pub fn reset(&mut self, window_size: usize) {
        self.val = 0;
        self.head.fill(-1);
        for i in 0..window_size {
            // `prev` is `unsigned short`; a window index above 65535 truncates.
            self.prev[i] = i as u16;
            self.hashval[i] = -1;
        }

        for slot in self.same.iter_mut().take(window_size) {
            *slot = 0;
        }

        self.val2 = 0;
        self.head2.fill(-1);
        for i in 0..window_size {
            self.prev2[i] = i as u16;
            self.hashval2[i] = -1;
        }
    }

    /// Update hash values at `pos`, matching `ZopfliUpdateHash`.
    pub fn update(&mut self, array: &[u8], pos: usize, end: usize) {
        // C stores `hpos` in `unsigned short`.
        let hpos = (pos & WINDOW_MASK) as u16;
        let hpos_us = hpos as usize;

        let c = if pos + MIN_MATCH <= end {
            array[pos + MIN_MATCH - 1]
        } else {
            0
        };
        self.update_hash_value(c);
        self.hashval[hpos_us] = self.val;
        let head_at = self.head[self.val as usize];
        if head_at != -1 && self.hashval[head_at as usize] == self.val {
            self.prev[hpos_us] = head_at as u16;
        } else {
            self.prev[hpos_us] = hpos;
        }
        self.head[self.val as usize] = i32::from(hpos);

        // Update "same". `(pos - 1)` wraps, matching `size_t` underflow.
        let mut amount: usize = 0;
        let prev_same = self.same[pos.wrapping_sub(1) & WINDOW_MASK];
        if prev_same > 1 {
            amount = usize::from(prev_same) - 1;
        }
        while pos + amount + 1 < end
            && array[pos] == array[pos + amount + 1]
            && amount < usize::from(u16::MAX)
        {
            amount += 1;
        }
        self.same[hpos_us] = amount as u16;

        // `unsigned short` promotes to signed `int` before the subtraction.
        let same_i = i32::from(self.same[hpos_us]);
        self.val2 = ((same_i - MIN_MATCH as i32) & 255) ^ self.val;
        self.hashval2[hpos_us] = self.val2;
        let head2_at = self.head2[self.val2 as usize];
        if head2_at != -1 && self.hashval2[head2_at as usize] == self.val2 {
            self.prev2[hpos_us] = head2_at as u16;
        } else {
            self.prev2[hpos_us] = hpos;
        }
        self.head2[self.val2 as usize] = i32::from(hpos);
    }

    /// Fill initial hash bytes, matching `ZopfliWarmupHash`.
    pub fn warmup(&mut self, array: &[u8], pos: usize, end: usize) {
        self.update_hash_value(array[pos]);
        if pos + 1 < end {
            self.update_hash_value(array[pos + 1]);
        }
    }

    fn update_hash_value(&mut self, c: u8) {
        self.val = ((self.val << HASH_SHIFT) ^ i32::from(c)) & HASH_MASK;
    }
}
