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

//! Optimal LZ77 parsing ("squeeze") from `src/zopfli/squeeze.c`.

use crate::deflate::calculate_block_size;
use crate::hash::ZopfliHash;
use crate::lz77::{find_longest_match, lz77_greedy, verify_len_dist, BlockState, Lz77Store};
use crate::symbols::{
    get_dist_extra_bits, get_dist_symbol, get_length_extra_bits, get_length_symbol,
};
use crate::tree::calculate_entropy;
use crate::util::{
    ZOPFLI_LARGE_FLOAT, ZOPFLI_MAX_MATCH, ZOPFLI_MIN_MATCH, ZOPFLI_NUM_D, ZOPFLI_NUM_LL,
    ZOPFLI_WINDOW_MASK, ZOPFLI_WINDOW_SIZE,
};

#[derive(Clone, Copy)]
struct SymbolStats {
    litlens: [usize; ZOPFLI_NUM_LL],
    dists: [usize; ZOPFLI_NUM_D],
    ll_symbols: [f64; ZOPFLI_NUM_LL],
    d_symbols: [f64; ZOPFLI_NUM_D],
}

impl SymbolStats {
    fn new() -> Self {
        Self {
            litlens: [0; ZOPFLI_NUM_LL],
            dists: [0; ZOPFLI_NUM_D],
            ll_symbols: [0.0; ZOPFLI_NUM_LL],
            d_symbols: [0.0; ZOPFLI_NUM_D],
        }
    }

    fn clear_freqs(&mut self) {
        self.litlens = [0; ZOPFLI_NUM_LL];
        self.dists = [0; ZOPFLI_NUM_D];
    }
}

fn add_weighed_stat_freqs(
    stats1: &SymbolStats,
    w1: f64,
    stats2: &SymbolStats,
    w2: f64,
    result: &mut SymbolStats,
) {
    for i in 0..ZOPFLI_NUM_LL {
        result.litlens[i] =
            (stats1.litlens[i] as f64 * w1 + stats2.litlens[i] as f64 * w2) as usize;
    }
    for i in 0..ZOPFLI_NUM_D {
        result.dists[i] = (stats1.dists[i] as f64 * w1 + stats2.dists[i] as f64 * w2) as usize;
    }
    result.litlens[256] = 1;
}

struct RanState {
    m_w: u32,
    m_z: u32,
}

impl RanState {
    fn new() -> Self {
        Self { m_w: 1, m_z: 2 }
    }

    /// Multiply-with-carry generator of G. Marsaglia.
    fn ran(&mut self) -> u32 {
        self.m_z = 36969u32
            .wrapping_mul(self.m_z & 65535)
            .wrapping_add(self.m_z >> 16);
        self.m_w = 18000u32
            .wrapping_mul(self.m_w & 65535)
            .wrapping_add(self.m_w >> 16);
        self.m_z.wrapping_shl(16).wrapping_add(self.m_w)
    }
}

fn randomize_freqs(state: &mut RanState, freqs: &mut [usize]) {
    let n = freqs.len();
    for i in 0..n {
        if (state.ran() >> 4) % 3 == 0 {
            freqs[i] = freqs[state.ran() as usize % n];
        }
    }
}

fn randomize_stat_freqs(state: &mut RanState, stats: &mut SymbolStats) {
    randomize_freqs(state, &mut stats.litlens);
    randomize_freqs(state, &mut stats.dists);
    stats.litlens[256] = 1;
}

fn get_cost_fixed(litlen: u32, dist: u32) -> f64 {
    if dist == 0 {
        if litlen <= 143 {
            8.0
        } else {
            9.0
        }
    } else {
        let dbits = get_dist_extra_bits(dist as i32);
        let lbits = get_length_extra_bits(litlen as i32);
        let lsym = get_length_symbol(litlen as i32);
        let mut cost = 0;
        if lsym <= 279 {
            cost += 7;
        } else {
            cost += 8;
        }
        cost += 5;
        f64::from(cost + dbits + lbits)
    }
}

fn get_cost_stat(litlen: u32, dist: u32, stats: &SymbolStats) -> f64 {
    if dist == 0 {
        stats.ll_symbols[litlen as usize]
    } else {
        let lsym = get_length_symbol(litlen as i32);
        let lbits = get_length_extra_bits(litlen as i32);
        let dsym = get_dist_symbol(dist as i32);
        let dbits = get_dist_extra_bits(dist as i32);
        f64::from(lbits + dbits) + stats.ll_symbols[lsym as usize] + stats.d_symbols[dsym as usize]
    }
}

fn get_cost_model_min_cost<F>(mut costmodel: F) -> f64
where
    F: FnMut(u32, u32) -> f64,
{
    const DSYMBOLS: [u32; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];

    let mut mincost = ZOPFLI_LARGE_FLOAT;
    let mut bestlength = 0u32;
    for i in 3..259 {
        let c = costmodel(i, 1);
        if c < mincost {
            bestlength = i;
            mincost = c;
        }
    }

    mincost = ZOPFLI_LARGE_FLOAT;
    let mut bestdist = 0u32;
    for &d in &DSYMBOLS {
        let c = costmodel(3, d);
        if c < mincost {
            bestdist = d;
            mincost = c;
        }
    }

    costmodel(bestlength, bestdist)
}

fn get_best_lengths<F>(
    s: &mut BlockState,
    input: &[u8],
    instart: usize,
    inend: usize,
    mut costmodel: F,
    length_array: &mut [u16],
    h: &mut ZopfliHash,
    costs: &mut [f32],
) -> f64
where
    F: FnMut(u32, u32) -> f64,
{
    let blocksize = inend - instart;
    if instart == inend {
        return 0.0;
    }

    let windowstart = instart.saturating_sub(ZOPFLI_WINDOW_SIZE);
    let mincost = get_cost_model_min_cost(&mut costmodel);

    h.reset(ZOPFLI_WINDOW_SIZE);
    h.warmup(input, windowstart, inend);
    for i in windowstart..instart {
        h.update(input, i, inend);
    }

    costs[1..=blocksize].fill(ZOPFLI_LARGE_FLOAT as f32);
    costs[0] = 0.0;
    length_array[0] = 0;

    let mut sublen = [0u16; 259];
    let mut i = instart;
    while i < inend {
        let mut j = i - instart;
        h.update(input, i, inend);

        // ZOPFLI_SHORTCUT_LONG_REPETITIONS
        if usize::from(h.same[i & ZOPFLI_WINDOW_MASK]) > ZOPFLI_MAX_MATCH * 2
            && i > instart + ZOPFLI_MAX_MATCH + 1
            && i + ZOPFLI_MAX_MATCH * 2 + 1 < inend
            && usize::from(h.same[(i - ZOPFLI_MAX_MATCH) & ZOPFLI_WINDOW_MASK]) > ZOPFLI_MAX_MATCH
        {
            let symbolcost = costmodel(ZOPFLI_MAX_MATCH as u32, 1);
            for _ in 0..ZOPFLI_MAX_MATCH {
                costs[j + ZOPFLI_MAX_MATCH] = (f64::from(costs[j]) + symbolcost) as f32;
                length_array[j + ZOPFLI_MAX_MATCH] = ZOPFLI_MAX_MATCH as u16;
                i += 1;
                j += 1;
                h.update(input, i, inend);
            }
        }

        let mut dist = 0u16;
        let mut leng = 0u16;
        find_longest_match(
            s,
            h,
            input,
            i,
            inend,
            ZOPFLI_MAX_MATCH,
            Some(&mut sublen),
            &mut dist,
            &mut leng,
        );

        if i + 1 <= inend {
            let new_cost = costmodel(u32::from(input[i]), 0) + f64::from(costs[j]);
            debug_assert!(new_cost >= 0.0);
            if new_cost < f64::from(costs[j + 1]) {
                costs[j + 1] = new_cost as f32;
                length_array[j + 1] = 1;
            }
        }

        let kend = (leng as usize).min(inend - i);
        let mincostaddcostj = mincost + f64::from(costs[j]);
        for k in 3..=kend {
            if f64::from(costs[j + k]) <= mincostaddcostj {
                continue;
            }
            let new_cost = costmodel(k as u32, u32::from(sublen[k])) + f64::from(costs[j]);
            debug_assert!(new_cost >= 0.0);
            if new_cost < f64::from(costs[j + k]) {
                debug_assert!(k <= ZOPFLI_MAX_MATCH);
                costs[j + k] = new_cost as f32;
                length_array[j + k] = k as u16;
            }
        }
        i += 1;
    }

    debug_assert!(costs[blocksize] >= 0.0);
    f64::from(costs[blocksize])
}

fn trace_backwards(size: usize, length_array: &[u16], path: &mut Vec<u16>) {
    path.clear();
    if size == 0 {
        return;
    }
    let mut index = size;
    loop {
        path.push(length_array[index]);
        debug_assert!(usize::from(length_array[index]) <= index);
        debug_assert!(usize::from(length_array[index]) <= ZOPFLI_MAX_MATCH);
        debug_assert_ne!(length_array[index], 0);
        index -= usize::from(length_array[index]);
        if index == 0 {
            break;
        }
    }
    path.reverse();
}

fn follow_path(
    s: &mut BlockState,
    input: &[u8],
    instart: usize,
    inend: usize,
    path: &[u16],
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

    let mut pos = instart;
    for &item in path {
        let mut length = item;
        debug_assert!(pos < inend);
        h.update(input, pos, inend);

        if usize::from(length) >= ZOPFLI_MIN_MATCH {
            let mut dummy_length = 0u16;
            let mut dist = 0u16;
            find_longest_match(
                s,
                h,
                input,
                pos,
                inend,
                usize::from(length),
                None,
                &mut dist,
                &mut dummy_length,
            );
            debug_assert!(!(dummy_length != length && length > 2 && dummy_length > 2));
            verify_len_dist(input, inend, pos, dist, length);
            store.store_lit_len_dist(length, dist, pos);
        } else {
            length = 1;
            store.store_lit_len_dist(u16::from(input[pos]), 0, pos);
        }

        debug_assert!(pos + usize::from(length) <= inend);
        for j in 1..usize::from(length) {
            h.update(input, pos + j, inend);
        }
        pos += usize::from(length);
    }
}

fn calculate_statistics(stats: &mut SymbolStats) {
    calculate_entropy(&stats.litlens, &mut stats.ll_symbols);
    calculate_entropy(&stats.dists, &mut stats.d_symbols);
}

fn get_statistics(store: &Lz77Store, stats: &mut SymbolStats) {
    for i in 0..store.size() {
        if store.dists[i] == 0 {
            stats.litlens[store.litlens[i] as usize] += 1;
        } else {
            stats.litlens[get_length_symbol(i32::from(store.litlens[i])) as usize] += 1;
            stats.dists[get_dist_symbol(i32::from(store.dists[i])) as usize] += 1;
        }
    }
    stats.litlens[256] = 1;
    calculate_statistics(stats);
}

fn lz77_optimal_run<F>(
    s: &mut BlockState,
    input: &[u8],
    instart: usize,
    inend: usize,
    path: &mut Vec<u16>,
    length_array: &mut [u16],
    costmodel: F,
    store: &mut Lz77Store,
    h: &mut ZopfliHash,
    costs: &mut [f32],
) -> f64
where
    F: FnMut(u32, u32) -> f64,
{
    let cost = get_best_lengths(s, input, instart, inend, costmodel, length_array, h, costs);
    path.clear();
    trace_backwards(inend - instart, length_array, path);
    follow_path(s, input, instart, inend, path, store, h);
    debug_assert!(cost < ZOPFLI_LARGE_FLOAT);
    cost
}

/// Calculates lit/len and dist pairs for given data.
///
/// If `instart` is larger than 0, it uses values before `instart` as starting
/// dictionary. Matching `ZopfliLZ77Optimal`.
pub fn lz77_optimal(
    s: &mut BlockState,
    input: &[u8],
    instart: usize,
    inend: usize,
    numiterations: i32,
    store: &mut Lz77Store,
) {
    let blocksize = inend - instart;
    let mut length_array = vec![0u16; blocksize + 1];
    let mut path = Vec::new();
    let mut currentstore = Lz77Store::new();
    let mut h = ZopfliHash::new();
    let mut stats = SymbolStats::new();
    let mut beststats = SymbolStats::new();
    let mut laststats;
    let mut costs = vec![0f32; blocksize + 1];
    let mut bestcost = ZOPFLI_LARGE_FLOAT;
    let mut lastcost = 0.0;
    let mut ran_state = RanState::new();
    let mut lastrandomstep = -1i32;

    lz77_greedy(s, input, instart, inend, &mut currentstore, &mut h);
    get_statistics(&currentstore, &mut stats);

    for i in 0..numiterations {
        currentstore.clear();
        lz77_optimal_run(
            s,
            input,
            instart,
            inend,
            &mut path,
            &mut length_array,
            |litlen, dist| get_cost_stat(litlen, dist, &stats),
            &mut currentstore,
            &mut h,
            &mut costs,
        );
        let cost = calculate_block_size(&currentstore, 0, currentstore.size(), 2);
        if s.options.verbose_more != 0 || (s.options.verbose != 0 && cost < bestcost) {
            eprintln!("Iteration {i}: {} bit", cost as i32);
        }
        if cost < bestcost {
            store.copy_from(&currentstore);
            beststats = stats;
            bestcost = cost;
        }
        laststats = stats;
        stats.clear_freqs();
        get_statistics(&currentstore, &mut stats);
        if lastrandomstep != -1 {
            let mut mixed = SymbolStats::new();
            add_weighed_stat_freqs(&stats, 1.0, &laststats, 0.5, &mut mixed);
            stats.litlens = mixed.litlens;
            stats.dists = mixed.dists;
            calculate_statistics(&mut stats);
        }
        if i > 5 && cost == lastcost {
            stats = beststats;
            randomize_stat_freqs(&mut ran_state, &mut stats);
            calculate_statistics(&mut stats);
            lastrandomstep = i;
        }
        lastcost = cost;
    }
}

/// Same as [`lz77_optimal`], but optimized for the fixed tree of the deflate
/// standard. Matching `ZopfliLZ77OptimalFixed`.
pub fn lz77_optimal_fixed(
    s: &mut BlockState,
    input: &[u8],
    instart: usize,
    inend: usize,
    store: &mut Lz77Store,
) {
    let blocksize = inend - instart;
    let mut length_array = vec![0u16; blocksize + 1];
    let mut path = Vec::new();
    let mut h = ZopfliHash::new();
    let mut costs = vec![0f32; blocksize + 1];

    s.blockstart = instart;
    s.blockend = inend;

    lz77_optimal_run(
        s,
        input,
        instart,
        inend,
        &mut path,
        &mut length_array,
        get_cost_fixed,
        store,
        &mut h,
        &mut costs,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;

    fn reconstruct(data: &[u8], instart: usize, store: &Lz77Store) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..store.size() {
            if store.dists[i] == 0 {
                out.push(store.litlens[i] as u8);
            } else {
                let dist = store.dists[i] as usize;
                let length = store.litlens[i] as usize;
                let pos = store.pos[i];
                for j in 0..length {
                    out.push(data[pos - dist + j]);
                }
            }
        }
        assert_eq!(out, data[instart..instart + out.len()]);
        out
    }

    #[test]
    fn marsaglia_rng_matches_c_seed() {
        let mut state = RanState::new();
        assert_eq!(state.m_w, 1);
        assert_eq!(state.m_z, 2);
        let first = state.ran();
        // 36969 * 2 = 73938; 18000 * 1 = 18000; (73938 << 16) + 18000
        assert_eq!(first, 73938u32.wrapping_shl(16).wrapping_add(18000));
    }

    #[test]
    fn empty_input_leaves_empty_store() {
        let opts = Options::default();
        let mut s = BlockState::new(opts, 0, 0, true);
        let mut store = Lz77Store::new();
        lz77_optimal(&mut s, b"", 0, 0, 5, &mut store);
        assert_eq!(store.size(), 0);

        let mut s = BlockState::new(opts, 0, 0, true);
        let mut store = Lz77Store::new();
        lz77_optimal_fixed(&mut s, b"", 0, 0, &mut store);
        assert_eq!(store.size(), 0);
    }

    #[test]
    fn optimal_roundtrips_repetitive_input() {
        let input = b"compressthis_foobar_foobar_foobar_foobar_foobar$";
        let opts = Options::default();
        let mut s = BlockState::new(opts, 0, input.len(), true);
        let mut store = Lz77Store::new();
        lz77_optimal(&mut s, input, 0, input.len(), 5, &mut store);
        let out = reconstruct(input, 0, &store);
        assert_eq!(out, input);
        assert!(store.size() < input.len(), "should find some matches");
    }

    #[test]
    fn optimal_fixed_roundtrips() {
        let input = b"the rain in spain falls mainly on the plain";
        let opts = Options::default();
        let mut s = BlockState::new(opts, 0, input.len(), true);
        let mut store = Lz77Store::new();
        lz77_optimal_fixed(&mut s, input, 0, input.len(), &mut store);
        let out = reconstruct(input, 0, &store);
        assert_eq!(out, input);
    }

    #[test]
    fn long_repetition_shortcut_roundtrips() {
        let mut input = vec![b'a'; 2000];
        input.extend_from_slice(b"unique-tail");
        let opts = Options::default();
        let mut s = BlockState::new(opts, 0, input.len(), true);
        let mut store = Lz77Store::new();
        lz77_optimal(&mut s, &input, 0, input.len(), 3, &mut store);
        let out = reconstruct(&input, 0, &store);
        assert_eq!(out, input);
    }

    #[test]
    fn dictionary_before_instart_is_used() {
        let input = b"abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
        let instart = 26;
        let opts = Options::default();
        let mut s = BlockState::new(opts, instart, input.len(), true);
        let mut store = Lz77Store::new();
        lz77_optimal(&mut s, input, instart, input.len(), 4, &mut store);
        let out = reconstruct(input, instart, &store);
        assert_eq!(out, &input[instart..]);
    }
}
