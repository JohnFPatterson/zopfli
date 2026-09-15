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

//! LZ77 store, longest match, and greedy parser (`src/zopfli/lz77.c`).

use crate::cache::LongestMatchCache;
use crate::hash::ZopfliHash;
use crate::symbols::{get_dist_symbol, get_length_symbol};
use crate::util::{
    ZOPFLI_MAX_CHAIN_HITS, ZOPFLI_MAX_MATCH, ZOPFLI_MIN_MATCH, ZOPFLI_NUM_D, ZOPFLI_NUM_LL,
    ZOPFLI_WINDOW_MASK, ZOPFLI_WINDOW_SIZE,
};
use crate::Options;

/// Stores lit/length and dist pairs for LZ77.
#[derive(Clone, Debug, Default)]
pub struct Lz77Store {
    pub litlens: Vec<u16>,
    pub dists: Vec<u16>,
    pub pos: Vec<usize>,
    pub ll_symbol: Vec<u16>,
    pub d_symbol: Vec<u16>,
    pub ll_counts: Vec<usize>,
    pub d_counts: Vec<usize>,
}

impl Lz77Store {
    /// Empty store, matching `ZopfliInitLZ77Store`.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn size(&self) -> usize {
        self.litlens.len()
    }

    pub fn clear(&mut self) {
        self.litlens.clear();
        self.dists.clear();
        self.pos.clear();
        self.ll_symbol.clear();
        self.d_symbol.clear();
        self.ll_counts.clear();
        self.d_counts.clear();
    }

    /// Copies `source` into `self`, matching `ZopfliCopyLZ77Store`.
    pub fn copy_from(&mut self, source: &Lz77Store) {
        *self = source.clone();
    }

    /// Appends a lit/len + dist pair, matching `ZopfliStoreLitLenDist`.
    pub fn store_lit_len_dist(&mut self, length: u16, dist: u16, pos: usize) {
        let origsize = self.size();
        let llstart = ZOPFLI_NUM_LL * (origsize / ZOPFLI_NUM_LL);
        let dstart = ZOPFLI_NUM_D * (origsize / ZOPFLI_NUM_D);

        if origsize % ZOPFLI_NUM_LL == 0 {
            if origsize == 0 {
                self.ll_counts.resize(ZOPFLI_NUM_LL, 0);
            } else {
                self.ll_counts
                    .extend_from_within(origsize - ZOPFLI_NUM_LL..origsize);
            }
        }
        if origsize % ZOPFLI_NUM_D == 0 {
            if origsize == 0 {
                self.d_counts.resize(ZOPFLI_NUM_D, 0);
            } else {
                self.d_counts
                    .extend_from_within(origsize - ZOPFLI_NUM_D..origsize);
            }
        }

        debug_assert!(length < 259);
        self.litlens.push(length);
        self.dists.push(dist);
        self.pos.push(pos);

        if dist == 0 {
            self.ll_symbol.push(length);
            self.d_symbol.push(0);
            self.ll_counts[llstart + length as usize] += 1;
        } else {
            let lsym = get_length_symbol(i32::from(length)) as u16;
            let dsym = get_dist_symbol(i32::from(dist)) as u16;
            self.ll_symbol.push(lsym);
            self.d_symbol.push(dsym);
            self.ll_counts[llstart + lsym as usize] += 1;
            self.d_counts[dstart + dsym as usize] += 1;
        }
    }

    /// Raw bytes spanned by LZ77 symbols `[lstart, lend)`.
    pub fn get_byte_range(&self, lstart: usize, lend: usize) -> usize {
        if lstart == lend {
            return 0;
        }
        let l = lend - 1;
        self.pos[l]
            + (if self.dists[l] == 0 {
                1
            } else {
                self.litlens[l] as usize
            })
            - self.pos[lstart]
    }

    fn get_histogram_at(&self, lpos: usize, ll_counts: &mut [usize], d_counts: &mut [usize]) {
        let llpos = ZOPFLI_NUM_LL * (lpos / ZOPFLI_NUM_LL);
        let dpos = ZOPFLI_NUM_D * (lpos / ZOPFLI_NUM_D);
        ll_counts.copy_from_slice(&self.ll_counts[llpos..llpos + ZOPFLI_NUM_LL]);
        let end = (llpos + ZOPFLI_NUM_LL).min(self.size());
        for i in (lpos + 1)..end {
            ll_counts[self.ll_symbol[i] as usize] -= 1;
        }
        d_counts.copy_from_slice(&self.d_counts[dpos..dpos + ZOPFLI_NUM_D]);
        let end = (dpos + ZOPFLI_NUM_D).min(self.size());
        for i in (lpos + 1)..end {
            if self.dists[i] != 0 {
                d_counts[self.d_symbol[i] as usize] -= 1;
            }
        }
    }

    /// Histogram of lit/len and dist symbols in `[lstart, lend)`.
    /// Does not add the end symbol 256.
    pub fn get_histogram(
        &self,
        lstart: usize,
        lend: usize,
        ll_counts: &mut [usize],
        d_counts: &mut [usize],
    ) {
        if lstart + ZOPFLI_NUM_LL * 3 > lend {
            ll_counts.fill(0);
            d_counts.fill(0);
            for i in lstart..lend {
                ll_counts[self.ll_symbol[i] as usize] += 1;
                if self.dists[i] != 0 {
                    d_counts[self.d_symbol[i] as usize] += 1;
                }
            }
        } else {
            self.get_histogram_at(lend - 1, ll_counts, d_counts);
            if lstart > 0 {
                let mut ll_counts2 = [0usize; ZOPFLI_NUM_LL];
                let mut d_counts2 = [0usize; ZOPFLI_NUM_D];
                self.get_histogram_at(lstart - 1, &mut ll_counts2, &mut d_counts2);
                for i in 0..ZOPFLI_NUM_LL {
                    ll_counts[i] -= ll_counts2[i];
                }
                for i in 0..ZOPFLI_NUM_D {
                    d_counts[i] -= d_counts2[i];
                }
            }
        }
    }
}

/// Some state information for compressing a block.
pub struct BlockState {
    pub options: Options,
    pub lmc: Option<LongestMatchCache>,
    pub blockstart: usize,
    pub blockend: usize,
}

impl BlockState {
    /// Matching `ZopfliInitBlockState`.
    pub fn new(options: Options, blockstart: usize, blockend: usize, add_lmc: bool) -> Self {
        Self {
            options,
            lmc: if add_lmc {
                Some(LongestMatchCache::new(blockend.saturating_sub(blockstart)))
            } else {
                None
            },
            blockstart,
            blockend,
        }
    }
}

fn get_length_score(length: i32, distance: i32) -> i32 {
    if distance > 1024 {
        length - 1
    } else {
        length
    }
}

/// Verifies that `length`/`dist` is a valid match. Used like C's assertion helper.
pub fn verify_len_dist(data: &[u8], datasize: usize, pos: usize, dist: u16, length: u16) {
    debug_assert!(pos + length as usize <= datasize);
    for i in 0..length as usize {
        debug_assert_eq!(data[pos - dist as usize + i], data[pos + i]);
    }
}

fn get_match(scan: &[u8], matched: &[u8], max_len: usize) -> usize {
    let n = max_len.min(scan.len()).min(matched.len());
    let mut i = 0;
    while i < n && scan[i] == matched[i] {
        i += 1;
    }
    i
}

fn try_get_from_longest_match_cache(
    s: &BlockState,
    pos: usize,
    limit: &mut usize,
    sublen: Option<&mut [u16]>,
    distance: &mut u16,
    length: &mut u16,
) -> bool {
    let Some(lmc) = s.lmc.as_ref() else {
        return false;
    };
    let lmcpos = pos - s.blockstart;
    let cache_available = lmc.length[lmcpos] == 0 || lmc.dist[lmcpos] != 0;
    let limit_ok_for_cache = cache_available
        && (*limit == ZOPFLI_MAX_MATCH
            || usize::from(lmc.length[lmcpos]) <= *limit
            || (sublen.is_some() && lmc.max_cached_sublen(lmcpos) as usize >= *limit));

    if limit_ok_for_cache && cache_available {
        if sublen.is_none() || u32::from(lmc.length[lmcpos]) <= lmc.max_cached_sublen(lmcpos) {
            *length = lmc.length[lmcpos];
            if usize::from(*length) > *limit {
                *length = *limit as u16;
            }
            if let Some(sublen) = sublen {
                lmc.cache_to_sublen(lmcpos, usize::from(*length), sublen);
                *distance = sublen[usize::from(*length)];
                debug_assert!(
                    !(*limit == ZOPFLI_MAX_MATCH && *length >= ZOPFLI_MIN_MATCH as u16)
                        || sublen[usize::from(*length)] == lmc.dist[lmcpos]
                );
            } else {
                *distance = lmc.dist[lmcpos];
            }
            return true;
        }
        *limit = usize::from(lmc.length[lmcpos]);
    }
    false
}

fn store_in_longest_match_cache(
    s: &mut BlockState,
    pos: usize,
    limit: usize,
    sublen: Option<&[u16]>,
    distance: u16,
    length: u16,
) {
    let Some(lmc) = s.lmc.as_mut() else {
        return;
    };
    let lmcpos = pos - s.blockstart;
    let cache_available = lmc.length[lmcpos] == 0 || lmc.dist[lmcpos] != 0;
    if limit == ZOPFLI_MAX_MATCH && sublen.is_some() && !cache_available {
        debug_assert!(lmc.length[lmcpos] == 1 && lmc.dist[lmcpos] == 0);
        lmc.dist[lmcpos] = if length < ZOPFLI_MIN_MATCH as u16 {
            0
        } else {
            distance
        };
        lmc.length[lmcpos] = if length < ZOPFLI_MIN_MATCH as u16 {
            0
        } else {
            length
        };
        debug_assert!(!(lmc.length[lmcpos] == 1 && lmc.dist[lmcpos] == 0));
        if let Some(sublen) = sublen {
            lmc.sublen_to_cache(sublen, lmcpos, usize::from(length));
        }
    }
}

/// Finds the longest match, matching `ZopfliFindLongestMatch`.
pub fn find_longest_match(
    s: &mut BlockState,
    h: &ZopfliHash,
    array: &[u8],
    pos: usize,
    size: usize,
    mut limit: usize,
    mut sublen: Option<&mut [u16]>,
    distance: &mut u16,
    length: &mut u16,
) {
    if try_get_from_longest_match_cache(s, pos, &mut limit, sublen.as_deref_mut(), distance, length)
    {
        debug_assert!(pos + usize::from(*length) <= size);
        return;
    }

    debug_assert!(limit <= ZOPFLI_MAX_MATCH);
    debug_assert!(limit >= ZOPFLI_MIN_MATCH);
    debug_assert!(pos < size);

    if size - pos < ZOPFLI_MIN_MATCH {
        *length = 0;
        *distance = 0;
        return;
    }

    if pos + limit > size {
        limit = size - pos;
    }

    let hpos = pos & ZOPFLI_WINDOW_MASK;
    let mut bestdist = 0u16;
    let mut bestlength = 1u16;
    let mut chain_counter = ZOPFLI_MAX_CHAIN_HITS as i32;

    let mut use_hash2 = false;
    let mut hval = h.val;
    debug_assert!(hval < 65536);

    let mut pp = h.head[hval as usize] as u16;
    let mut p = h.prev[pp as usize];
    debug_assert_eq!(pp as usize, hpos);

    let mut dist = if p < pp {
        u32::from(pp) - u32::from(p)
    } else {
        (ZOPFLI_WINDOW_SIZE as u32 - u32::from(p)) + u32::from(pp)
    };

    while dist < ZOPFLI_WINDOW_SIZE as u32 {
        debug_assert!((p as usize) < ZOPFLI_WINDOW_SIZE);
        debug_assert_eq!(
            p,
            if use_hash2 {
                h.prev2[pp as usize]
            } else {
                h.prev[pp as usize]
            }
        );
        let hashval_p = if use_hash2 {
            h.hashval2[p as usize]
        } else {
            h.hashval[p as usize]
        };
        debug_assert_eq!(hashval_p, hval);

        let mut currentlength = 0u16;
        if dist > 0 {
            debug_assert!(pos < size);
            debug_assert!((dist as usize) <= pos);
            let scan = pos;
            let matched = pos - dist as usize;

            if pos + bestlength as usize >= size
                || array[scan + bestlength as usize] == array[matched + bestlength as usize]
            {
                let mut scan_off = scan;
                let mut match_off = matched;
                let same0 = h.same[pos & ZOPFLI_WINDOW_MASK];
                if same0 > 2 && array[scan] == array[matched] {
                    let same1 = h.same[(pos - dist as usize) & ZOPFLI_WINDOW_MASK];
                    let mut same = same0.min(same1);
                    if usize::from(same) > limit {
                        same = limit as u16;
                    }
                    scan_off += usize::from(same);
                    match_off += usize::from(same);
                }
                let remaining = (pos + limit).saturating_sub(scan_off);
                let n = get_match(&array[scan_off..], &array[match_off..], remaining);
                currentlength = ((scan_off + n) - pos) as u16;
            }

            if currentlength > bestlength {
                if let Some(ref mut sublen) = sublen {
                    for j in (bestlength as usize + 1)..=currentlength as usize {
                        sublen[j] = dist as u16;
                    }
                }
                bestdist = dist as u16;
                bestlength = currentlength;
                if currentlength as usize >= limit {
                    break;
                }
            }
        }

        if !use_hash2
            && bestlength as usize >= usize::from(h.same[hpos])
            && h.val2 == h.hashval2[p as usize]
        {
            use_hash2 = true;
            hval = h.val2;
        }

        pp = p;
        p = if use_hash2 {
            h.prev2[p as usize]
        } else {
            h.prev[p as usize]
        };
        if p == pp {
            break;
        }

        dist += if p < pp {
            u32::from(pp) - u32::from(p)
        } else {
            (ZOPFLI_WINDOW_SIZE as u32 - u32::from(p)) + u32::from(pp)
        };

        chain_counter -= 1;
        if chain_counter <= 0 {
            break;
        }
    }

    store_in_longest_match_cache(s, pos, limit, sublen.as_deref(), bestdist, bestlength);

    debug_assert!(bestlength as usize <= limit);
    *distance = bestdist;
    *length = bestlength;
    debug_assert!(pos + usize::from(*length) <= size);
}

/// Greedy LZ77 with lazy matching, matching `ZopfliLZ77Greedy`.
pub fn lz77_greedy(
    s: &mut BlockState,
    input: &[u8],
    instart: usize,
    inend: usize,
    store: &mut Lz77Store,
    h: &mut ZopfliHash,
) {
    if instart == inend {
        return;
    }

    let windowstart = instart.saturating_sub(ZOPFLI_WINDOW_SIZE);
    h.reset(ZOPFLI_WINDOW_SIZE);
    h.warmup(input, windowstart, inend);
    for i in windowstart..instart {
        h.update(input, i, inend);
    }

    let mut dummysublen = [0u16; 259];
    let mut prev_length = 0u32;
    let mut prev_match = 0u32;
    let mut match_available = false;

    let mut i = instart;
    while i < inend {
        h.update(input, i, inend);

        let mut dist = 0u16;
        let mut leng = 0u16;
        find_longest_match(
            s,
            h,
            input,
            i,
            inend,
            ZOPFLI_MAX_MATCH,
            Some(&mut dummysublen),
            &mut dist,
            &mut leng,
        );
        let lengthscore = get_length_score(i32::from(leng), i32::from(dist));

        let prevlengthscore = get_length_score(prev_length as i32, prev_match as i32);
        if match_available {
            match_available = false;
            if lengthscore > prevlengthscore + 1 {
                store.store_lit_len_dist(u16::from(input[i - 1]), 0, i - 1);
                if lengthscore >= ZOPFLI_MIN_MATCH as i32 && (leng as usize) < ZOPFLI_MAX_MATCH {
                    match_available = true;
                    prev_length = u32::from(leng);
                    prev_match = u32::from(dist);
                    i += 1;
                    continue;
                }
            } else {
                leng = prev_length as u16;
                dist = prev_match as u16;
                let _lengthscore = prevlengthscore;
                verify_len_dist(input, inend, i - 1, dist, leng);
                store.store_lit_len_dist(leng, dist, i - 1);
                for _ in 2..leng {
                    debug_assert!(i < inend);
                    i += 1;
                    h.update(input, i, inend);
                }
                i += 1;
                continue;
            }
        } else if lengthscore >= ZOPFLI_MIN_MATCH as i32 && (leng as usize) < ZOPFLI_MAX_MATCH {
            match_available = true;
            prev_length = u32::from(leng);
            prev_match = u32::from(dist);
            i += 1;
            continue;
        }

        if lengthscore >= ZOPFLI_MIN_MATCH as i32 {
            verify_len_dist(input, inend, i, dist, leng);
            store.store_lit_len_dist(leng, dist, i);
        } else {
            leng = 1;
            store.store_lit_len_dist(u16::from(input[i]), 0, i);
        }
        for _ in 1..leng {
            debug_assert!(i < inend);
            i += 1;
            h.update(input, i, inend);
        }
        i += 1;
    }
}
