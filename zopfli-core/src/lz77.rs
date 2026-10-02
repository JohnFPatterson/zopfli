//! LZ77 store, longest-match search, and greedy parsing.
//!
//! Integer widths follow the C sources: `unsigned short` values (`hpos`, `p`,
//! `pp`, lengths, distances stored in the window) are `u16` and promote to
//! signed `i32` in mixed arithmetic. Chain distances accumulate in `u32`
//! (`unsigned`).

use crate::cache::LongestMatchCache;
use crate::consts::{
    MAX_CHAIN_HITS, MAX_MATCH, MIN_MATCH, NUM_D, NUM_LL, WINDOW_MASK, WINDOW_SIZE,
};
use crate::hash::Hash;
use crate::symbols::*;
use crate::Options;

/// Lit/length and distance pairs for one LZ77 stream.
///
/// `dists[i] == 0` means `litlens[i]` is a literal byte. Otherwise `litlens[i]`
/// is a match length and `dists[i]` is the distance. The five symbol vectors
/// stay the same length. Cumulative histograms are stored in chunks of
/// [`NUM_LL`] and [`NUM_D`], not one full histogram per symbol.
#[derive(Clone, Debug)]
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
    /// Empty store. Every vector has length 0.
    pub fn new() -> Self {
        Self {
            litlens: Vec::new(),
            dists: Vec::new(),
            pos: Vec::new(),
            ll_symbol: Vec::new(),
            d_symbol: Vec::new(),
            ll_counts: Vec::new(),
            d_counts: Vec::new(),
        }
    }

    /// Number of lit/len/dist commands. Equals `litlens.len()`.
    pub fn size(&self) -> usize {
        self.litlens.len()
    }

    /// Replace `self` with a copy of `source`, including histogram vectors.
    ///
    /// An empty source copies nothing (`CeilDiv(0, n) == 0`).
    pub fn copy_from(&mut self, source: &Lz77Store) {
        self.litlens.clone_from(&source.litlens);
        self.dists.clone_from(&source.dists);
        self.pos.clone_from(&source.pos);
        self.ll_symbol.clone_from(&source.ll_symbol);
        self.d_symbol.clone_from(&source.d_symbol);
        self.ll_counts.clone_from(&source.ll_counts);
        self.d_counts.clone_from(&source.d_counts);
    }

    /// Append one literal (`dist == 0`) or match and update the rolling histograms.
    ///
    /// When `origsize` is a multiple of [`NUM_LL`] (including 0), a new lit/len
    /// histogram chunk is appended first: zeros if `origsize == 0`, otherwise a
    /// copy of the previous chunk. Distances use [`NUM_D`] the same way. Symbol
    /// codes from [`length_symbol`] / [`dist_symbol`] are `i32` values stored as
    /// `u16`.
    pub fn store_lit_len_dist(&mut self, length: u16, dist: u16, pos: usize) {
        let origsize = self.size();
        let llstart = NUM_LL * (origsize / NUM_LL);
        let dstart = NUM_D * (origsize / NUM_D);

        if origsize % NUM_LL == 0 {
            if origsize == 0 {
                self.ll_counts.resize(NUM_LL, 0);
            } else {
                self.ll_counts
                    .extend_from_within((origsize - NUM_LL)..origsize);
            }
        }
        if origsize % NUM_D == 0 {
            if origsize == 0 {
                self.d_counts.resize(NUM_D, 0);
            } else {
                self.d_counts
                    .extend_from_within((origsize - NUM_D)..origsize);
            }
        }

        self.litlens.push(length);
        self.dists.push(dist);
        self.pos.push(pos);
        debug_assert!(length < 259);

        if dist == 0 {
            self.ll_symbol.push(length);
            self.d_symbol.push(0);
            self.ll_counts[llstart + usize::from(length)] =
                self.ll_counts[llstart + usize::from(length)].wrapping_add(1);
        } else {
            let ll_sym = length_symbol(i32::from(length));
            let d_sym = dist_symbol(i32::from(dist));
            self.ll_symbol.push(ll_sym as u16);
            self.d_symbol.push(d_sym as u16);
            let ll_index = llstart + (ll_sym as usize);
            let d_index = dstart + (d_sym as usize);
            self.ll_counts[ll_index] = self.ll_counts[ll_index].wrapping_add(1);
            self.d_counts[d_index] = self.d_counts[d_index].wrapping_add(1);
        }
    }

    /// Append every command of `source` via [`Self::store_lit_len_dist`].
    pub fn append_store(&mut self, source: &Lz77Store) {
        for i in 0..source.size() {
            self.store_lit_len_dist(source.litlens[i], source.dists[i], source.pos[i]);
        }
    }

    /// Uncompressed byte length covered by commands in `[lstart, lend)`.
    pub fn byte_range(&self, lstart: usize, lend: usize) -> usize {
        if lstart == lend {
            return 0;
        }
        let l = lend - 1;
        let add = if self.dists[l] == 0 {
            1
        } else {
            usize::from(self.litlens[l])
        };
        self.pos[l].wrapping_add(add).wrapping_sub(self.pos[lstart])
    }

    /// Histogram of lit/len and distance symbols in `[lstart, lend)`.
    ///
    /// `ll_counts` has length [`NUM_LL`] and `d_counts` has length [`NUM_D`].
    /// Ranges with `lstart + NUM_LL * 3 > lend` are summed directly. Longer
    /// ranges subtract the cumulative histograms at the two ends.
    pub fn histogram(
        &self,
        lstart: usize,
        lend: usize,
        ll_counts: &mut [usize],
        d_counts: &mut [usize],
    ) {
        if lstart.wrapping_add(NUM_LL * 3) > lend {
            for count in ll_counts.iter_mut().take(NUM_LL) {
                *count = 0;
            }
            for count in d_counts.iter_mut().take(NUM_D) {
                *count = 0;
            }
            for i in lstart..lend {
                let sym = usize::from(self.ll_symbol[i]);
                ll_counts[sym] = ll_counts[sym].wrapping_add(1);
                if self.dists[i] != 0 {
                    let dsym = usize::from(self.d_symbol[i]);
                    d_counts[dsym] = d_counts[dsym].wrapping_add(1);
                }
            }
        } else {
            self.histogram_at(lend - 1, ll_counts, d_counts);
            if lstart > 0 {
                let mut ll2 = [0usize; NUM_LL];
                let mut d2 = [0usize; NUM_D];
                self.histogram_at(lstart - 1, &mut ll2, &mut d2);
                for i in 0..NUM_LL {
                    ll_counts[i] = ll_counts[i].wrapping_sub(ll2[i]);
                }
                for i in 0..NUM_D {
                    d_counts[i] = d_counts[i].wrapping_sub(d2[i]);
                }
            }
        }
    }

    /// Cumulative histogram of symbols `[0, lpos]`, with the tail of the
    /// current chunk subtracted back out.
    fn histogram_at(&self, lpos: usize, ll_counts: &mut [usize], d_counts: &mut [usize]) {
        let llpos = NUM_LL * (lpos / NUM_LL);
        let dpos = NUM_D * (lpos / NUM_D);
        ll_counts[..NUM_LL].copy_from_slice(&self.ll_counts[llpos..llpos + NUM_LL]);
        let ll_end = (llpos + NUM_LL).min(self.size());
        for i in (lpos + 1)..ll_end {
            let sym = usize::from(self.ll_symbol[i]);
            ll_counts[sym] = ll_counts[sym].wrapping_sub(1);
        }
        d_counts[..NUM_D].copy_from_slice(&self.d_counts[dpos..dpos + NUM_D]);
        let d_end = (dpos + NUM_D).min(self.size());
        for i in (lpos + 1)..d_end {
            if self.dists[i] != 0 {
                let sym = usize::from(self.d_symbol[i]);
                d_counts[sym] = d_counts[sym].wrapping_sub(1);
            }
        }
    }
}

impl Default for Lz77Store {
    fn default() -> Self {
        Self::new()
    }
}

/// State for compressing one block. `options` is an owned copy.
pub struct BlockState {
    pub options: Options,
    pub lmc: Option<LongestMatchCache>,
    pub blockstart: usize,
    pub blockend: usize,
}

impl BlockState {
    /// When `add_lmc` is set, the cache covers `blockend - blockstart` positions.
    pub fn new(options: Options, blockstart: usize, blockend: usize, add_lmc: bool) -> Self {
        let lmc = if add_lmc {
            Some(LongestMatchCache::new(blockend - blockstart))
        } else {
            None
        };
        Self {
            options,
            lmc,
            blockstart,
            blockend,
        }
    }
}

/// Step between two `unsigned short` window indices.
///
/// C promotes `p` and `pp` to signed `int` before subtracting from
/// `WINDOW_SIZE`, then stores the sum in `unsigned`. A negative `int` becomes
/// a wrapped `u32`.
fn window_dist(p: u16, pp: u16) -> u32 {
    if p < pp {
        u32::from(pp - p)
    } else {
        let delta = (WINDOW_SIZE as i32) - i32::from(p) + i32::from(pp);
        delta as u32
    }
}

/// Bytes of `scan` that equal `earlier`, stopping at `scan.len()`.
///
/// The C search compares eight bytes at a time while the cursor is at least
/// eight bytes before the end. Equality of those bytes matches a byte loop.
fn match_length(scan: &[u8], earlier: &[u8]) -> usize {
    let max_len = scan.len().min(earlier.len());
    let safe = max_len.saturating_sub(8);
    let mut i = 0;
    while i < safe {
        if scan[i..i + 8] != earlier[i..i + 8] {
            break;
        }
        i += 8;
    }
    while i != max_len && scan[i] == earlier[i] {
        i += 1;
    }
    i
}

/// Greedy length score. Long distances cost one point so length 3 is avoided.
fn get_length_score(length: i32, distance: i32) -> i32 {
    if distance > 1024 {
        length - 1
    } else {
        length
    }
}

/// Debug check that `data[pos..pos+length]` equals the bytes `dist` earlier.
fn verify_len_dist(data: &[u8], datasize: usize, pos: usize, dist: u16, length: u16) {
    debug_assert!(pos.wrapping_add(usize::from(length)) <= datasize);
    let mut i = 0usize;
    while i < usize::from(length) {
        let left = data[pos.wrapping_sub(usize::from(dist)).wrapping_add(i)];
        let right = data[pos.wrapping_add(i)];
        if left != right {
            debug_assert_eq!(left, right);
            break;
        }
        i += 1;
    }
}

/// Read a cached match. Returns true when `distance` and `length` were filled.
///
/// `limit` may be lowered when the cache knows the best length but not every
/// sub-length. `sublen == None` is the C NULL pointer. Cache index is
/// `pos - blockstart`.
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
    let cached_length = lmc.length[lmcpos];
    let cached_dist = lmc.dist[lmcpos];
    // Length > 0 and dist 0 is the "not filled in yet" marker.
    let cache_available = cached_length == 0 || cached_dist != 0;
    let max_sub = lmc.max_cached_sublen(lmcpos, usize::from(cached_length));
    let limit_ok_for_cache = cache_available
        && (*limit == MAX_MATCH
            || usize::from(cached_length) <= *limit
            || (sublen.is_some() && (max_sub as usize) >= *limit));

    if limit_ok_for_cache && cache_available {
        if sublen.is_none() || u32::from(cached_length) <= max_sub {
            *length = cached_length;
            if usize::from(*length) > *limit {
                *length = *limit as u16;
            }
            if let Some(sublen) = sublen {
                lmc.cache_to_sublen(lmcpos, usize::from(*length), sublen);
                *distance = sublen[usize::from(*length)];
                if *limit == MAX_MATCH && usize::from(*length) >= MIN_MATCH {
                    debug_assert_eq!(sublen[usize::from(*length)], cached_dist);
                }
            } else {
                *distance = cached_dist;
            }
            return true;
        }
        // Sub-lengths still have to be computed, but the search can stop early.
        *limit = usize::from(cached_length);
    }
    false
}

/// Store a full-limit match in the cache. Shorter limits and cache hits are skipped.
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
    let Some(sublen) = sublen else {
        return;
    };
    if limit == MAX_MATCH && !cache_available {
        debug_assert!(lmc.length[lmcpos] == 1 && lmc.dist[lmcpos] == 0);
        if usize::from(length) < MIN_MATCH {
            lmc.dist[lmcpos] = 0;
            lmc.length[lmcpos] = 0;
        } else {
            lmc.dist[lmcpos] = distance;
            lmc.length[lmcpos] = length;
        }
        debug_assert!(!(lmc.length[lmcpos] == 1 && lmc.dist[lmcpos] == 0));
        lmc.sublen_to_cache(sublen, lmcpos, usize::from(length));
    }
}

/// Longest match at `pos` in `array[..size]`, capped at `limit`.
///
/// `sublen`, when present, is 259 entries indexed by length. This function
/// writes `sublen[j]` for `j` in `bestlength + 1 ..= currentlength` each time
/// a longer match is found (`bestlength` starts at 1, so index 0 is never
/// written here). Indices `3..=length` are therefore filled before a later
/// cache store reads them. `distance` and `length` receive the best pair.
/// A length of 1 with distance 0 means no match (callers treat that as a literal).
#[allow(clippy::too_many_arguments)]
pub fn find_longest_match(
    s: &mut BlockState,
    h: &Hash,
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
        debug_assert!(pos.wrapping_add(usize::from(*length)) <= size);
        return;
    }

    debug_assert!(limit <= MAX_MATCH);
    debug_assert!(limit >= MIN_MATCH);
    debug_assert!(pos < size);

    if size - pos < MIN_MATCH {
        *length = 0;
        *distance = 0;
        return;
    }

    if pos + limit > size {
        limit = size - pos;
    }

    let hpos: u16 = (pos & WINDOW_MASK) as u16;
    debug_assert!(h.val < 65536);
    let mut hval = h.val;
    let mut use_second = false;

    // `head` stores a window index as `int`. Narrowing matches assignment to
    // `unsigned short` (so -1 becomes 65535).
    let mut pp: u16 = h.head[hval as usize] as u16;
    let mut p: u16 = h.prev[usize::from(pp)];
    debug_assert_eq!(pp, hpos);

    let mut dist: u32 = window_dist(p, pp);
    let mut bestdist: u16 = 0;
    let mut bestlength: u16 = 1;
    // `MAX_CHAIN_HITS < WINDOW_SIZE`, so the chain limit is compiled in.
    let mut chain_counter: i32 = MAX_CHAIN_HITS as i32;

    while (dist as usize) < WINDOW_SIZE {
        let mut currentlength: u16 = 0;

        debug_assert!((p as usize) < WINDOW_SIZE);
        debug_assert_eq!(
            p,
            if use_second {
                h.prev2[usize::from(pp)]
            } else {
                h.prev[usize::from(pp)]
            }
        );
        debug_assert_eq!(
            if use_second {
                h.hashval2[usize::from(p)]
            } else {
                h.hashval[usize::from(p)]
            },
            hval
        );

        if dist > 0 {
            debug_assert!(pos < size);
            debug_assert!((dist as usize) <= pos);
            let mut scan = pos;
            let mut match_pos = pos - (dist as usize);

            if pos + usize::from(bestlength) >= size
                || array[scan + usize::from(bestlength)]
                    == array[match_pos + usize::from(bestlength)]
            {
                let same0 = h.same[pos & WINDOW_MASK];
                if same0 > 2 && array[scan] == array[match_pos] {
                    let same1 = h.same[(pos - (dist as usize)) & WINDOW_MASK];
                    let mut same = if same0 < same1 { same0 } else { same1 };
                    if usize::from(same) > limit {
                        same = limit as u16;
                    }
                    scan += usize::from(same);
                    match_pos += usize::from(same);
                }
                let array_end = pos + limit;
                let found = match_length(&array[scan..array_end], &array[match_pos..array_end]);
                scan += found;
                currentlength = (scan - pos) as u16;
            }

            if currentlength > bestlength {
                if let Some(slots) = sublen.as_deref_mut() {
                    // Starts at bestlength + 1 (initially 2). Successive
                    // improvements fill 3..=length, which the cache later reads.
                    let mut j = usize::from(bestlength) + 1;
                    let end = usize::from(currentlength);
                    while j <= end {
                        slots[j] = dist as u16;
                        j += 1;
                    }
                }
                bestdist = dist as u16;
                bestlength = currentlength;
                if usize::from(currentlength) >= limit {
                    break;
                }
            }
        }

        // Switch once to the length-and-first-byte hash.
        if !use_second
            && bestlength >= h.same[usize::from(hpos)]
            && h.val2 == h.hashval2[usize::from(p)]
        {
            use_second = true;
            hval = h.val2;
        }

        pp = p;
        p = if use_second {
            h.prev2[usize::from(p)]
        } else {
            h.prev[usize::from(p)]
        };
        if p == pp {
            break;
        }

        dist = dist.wrapping_add(window_dist(p, pp));
        chain_counter -= 1;
        if chain_counter <= 0 {
            break;
        }
    }

    store_in_longest_match_cache(s, pos, limit, sublen.as_deref(), bestdist, bestlength);

    debug_assert!(usize::from(bestlength) <= limit);
    *distance = bestdist;
    *length = bestlength;
    debug_assert!(pos.wrapping_add(usize::from(*length)) <= size);
}

/// Greedy LZ77 with lazy matching. Bytes before `instart` are the dictionary.
pub fn lz77_greedy(
    s: &mut BlockState,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    store: &mut Lz77Store,
    h: &mut Hash,
) {
    if instart == inend {
        return;
    }

    let windowstart = if instart > WINDOW_SIZE {
        instart - WINDOW_SIZE
    } else {
        0
    };
    // Rust requires initialization. C leaves this uninitialized and only reads
    // indices the matcher has written (`3..=length` once a match exists).
    let mut dummysublen = [0u16; 259];

    // Lazy matching. Previous length and distance are `unsigned` (`u32`).
    let mut prev_length: u32 = 0;
    let mut prev_match: u32 = 0;
    let mut match_available = false;

    h.reset(WINDOW_SIZE);
    h.warmup(in_data, windowstart, inend);
    for i in windowstart..instart {
        h.update(in_data, i, inend);
    }

    let mut i = instart;
    while i < inend {
        h.update(in_data, i, inend);

        let mut dist: u16 = 0;
        let mut leng: u16 = 0;
        find_longest_match(
            s,
            h,
            in_data,
            i,
            inend,
            MAX_MATCH,
            Some(&mut dummysublen),
            &mut dist,
            &mut leng,
        );
        let lengthscore = get_length_score(i32::from(leng), i32::from(dist));
        let prevlengthscore = get_length_score(prev_length as i32, prev_match as i32);

        if match_available {
            match_available = false;
            if lengthscore > prevlengthscore + 1 {
                store.store_lit_len_dist(u16::from(in_data[i - 1]), 0, i - 1);
                if lengthscore >= MIN_MATCH as i32 && usize::from(leng) < MAX_MATCH {
                    match_available = true;
                    prev_length = u32::from(leng);
                    prev_match = u32::from(dist);
                    i += 1;
                    continue;
                }
            } else {
                leng = prev_length as u16;
                dist = prev_match as u16;
                verify_len_dist(in_data, inend, i - 1, dist, leng);
                store.store_lit_len_dist(leng, dist, i - 1);
                let mut j = 2usize;
                while j < usize::from(leng) {
                    debug_assert!(i < inend);
                    i += 1;
                    h.update(in_data, i, inend);
                    j += 1;
                }
                i += 1;
                continue;
            }
        } else if lengthscore >= MIN_MATCH as i32 && usize::from(leng) < MAX_MATCH {
            match_available = true;
            prev_length = u32::from(leng);
            prev_match = u32::from(dist);
            i += 1;
            continue;
        }

        if lengthscore >= MIN_MATCH as i32 {
            verify_len_dist(in_data, inend, i, dist, leng);
            store.store_lit_len_dist(leng, dist, i);
        } else {
            leng = 1;
            store.store_lit_len_dist(u16::from(in_data[i]), 0, i);
        }
        let mut j = 1usize;
        while j < usize::from(leng) {
            debug_assert!(i < inend);
            i += 1;
            h.update(in_data, i, inend);
            j += 1;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_store_and_empty_copy() {
        let store = Lz77Store::new();
        assert_eq!(store.size(), 0);
        assert!(store.litlens.is_empty());
        assert!(store.dists.is_empty());
        assert!(store.pos.is_empty());
        assert!(store.ll_symbol.is_empty());
        assert!(store.d_symbol.is_empty());
        assert!(store.ll_counts.is_empty());
        assert!(store.d_counts.is_empty());

        let mut dest = Lz77Store::new();
        dest.store_lit_len_dist(1, 0, 0);
        dest.copy_from(&store);
        assert_eq!(dest.size(), 0);
        assert!(dest.ll_counts.is_empty());
        assert!(dest.d_counts.is_empty());
    }

    #[test]
    fn histogram_chunks_and_symbols() {
        let mut store = Lz77Store::new();
        store.store_lit_len_dist(7, 0, 4);
        assert_eq!(store.size(), 1);
        assert_eq!(store.litlens.len(), store.dists.len());
        assert_eq!(store.litlens.len(), store.pos.len());
        assert_eq!(store.litlens.len(), store.ll_symbol.len());
        assert_eq!(store.litlens.len(), store.d_symbol.len());
        assert_eq!(store.ll_counts.len(), NUM_LL);
        assert_eq!(store.d_counts.len(), NUM_D);
        assert_eq!(store.ll_symbol[0], 7);
        assert_eq!(store.d_symbol[0], 0);
        assert_eq!(store.ll_counts[7], 1);
        assert_eq!(store.byte_range(0, 0), 0);
        assert_eq!(store.byte_range(0, 1), 1);

        for i in 1..NUM_D {
            store.store_lit_len_dist(1, 0, 4 + i);
        }
        assert_eq!(store.d_counts.len(), NUM_D);
        store.store_lit_len_dist(3, 1, 4 + NUM_D);
        assert_eq!(store.size(), NUM_D + 1);
        assert_eq!(store.d_counts.len(), NUM_D * 2);
        let d_sym = dist_symbol(1) as u16;
        assert_eq!(store.d_symbol[NUM_D], d_sym);
        assert_eq!(store.ll_symbol[NUM_D], length_symbol(3) as u16);
        assert_eq!(store.d_counts[NUM_D + usize::from(d_sym)], 1);
        // Previous distance chunk was copied (still zeros) before the increment.
        assert!(store.d_counts[..NUM_D].iter().all(|&c| c == 0));
    }

    #[test]
    fn histogram_small_and_subtract_paths_agree() {
        let mut store = Lz77Store::new();
        let n = NUM_LL * 3;
        for i in 0..n {
            let lit = (i % 17) as u16;
            store.store_lit_len_dist(lit, 0, i);
        }
        store.store_lit_len_dist(4, 2, n);
        let total = store.size();

        let mut small_ll = [0usize; NUM_LL];
        let mut small_d = [0usize; NUM_D];
        store.histogram(0, 10, &mut small_ll, &mut small_d);
        assert_eq!(small_ll.iter().sum::<usize>(), 10);
        assert_eq!(small_d.iter().sum::<usize>(), 0);

        let mut full_ll = [0usize; NUM_LL];
        let mut full_d = [0usize; NUM_D];
        store.histogram(0, total, &mut full_ll, &mut full_d);

        let mut manual_ll = [0usize; NUM_LL];
        let mut manual_d = [0usize; NUM_D];
        for i in 0..total {
            manual_ll[usize::from(store.ll_symbol[i])] += 1;
            if store.dists[i] != 0 {
                manual_d[usize::from(store.d_symbol[i])] += 1;
            }
        }
        assert_eq!(full_ll, manual_ll);
        assert_eq!(full_d, manual_d);

        // `1 + NUM_LL * 3 == total`, so this is the subtract path with lstart > 0.
        let mut mid_ll = [0usize; NUM_LL];
        let mut mid_d = [0usize; NUM_D];
        store.histogram(1, total, &mut mid_ll, &mut mid_d);
        let mut mid_manual_ll = [0usize; NUM_LL];
        let mut mid_manual_d = [0usize; NUM_D];
        for i in 1..total {
            mid_manual_ll[usize::from(store.ll_symbol[i])] += 1;
            if store.dists[i] != 0 {
                mid_manual_d[usize::from(store.d_symbol[i])] += 1;
            }
        }
        assert_eq!(mid_ll, mid_manual_ll);
        assert_eq!(mid_d, mid_manual_d);
        assert_eq!(store.byte_range(0, total), n + 4);
    }

    #[test]
    fn append_and_copy_keep_histograms() {
        let mut a = Lz77Store::new();
        a.store_lit_len_dist(9, 0, 0);
        a.store_lit_len_dist(5, 1, 1);
        let mut b = Lz77Store::new();
        b.append_store(&a);
        assert_eq!(b.litlens, a.litlens);
        assert_eq!(b.dists, a.dists);
        assert_eq!(b.pos, a.pos);
        assert_eq!(b.ll_symbol, a.ll_symbol);
        assert_eq!(b.d_symbol, a.d_symbol);
        assert_eq!(b.ll_counts, a.ll_counts);
        assert_eq!(b.d_counts, a.d_counts);

        let mut c = Lz77Store::new();
        c.copy_from(&b);
        assert_eq!(c.ll_counts, b.ll_counts);
        assert_eq!(c.size(), 2);
    }

    #[test]
    fn window_dist_promotes_unsigned_short_to_signed() {
        assert_eq!(window_dist(0, 1), 1);
        assert_eq!(window_dist(1, 0), (WINDOW_SIZE as u32) - 1);
        assert_eq!(window_dist(100, 50), (WINDOW_SIZE as u32) - 100 + 50);
        // 32768 - 65535 = -32767, then converted to unsigned.
        assert_eq!(window_dist(65535, 0), (-32767i32) as u32);
        assert_eq!(get_length_score(3, 1025), 2);
        assert_eq!(get_length_score(3, 1024), 3);
        assert_eq!(get_length_score(0, 5000), -1);
    }
}
