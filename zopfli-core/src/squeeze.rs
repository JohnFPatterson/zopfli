//! Optimal LZ77 parsing (`src/zopfli/squeeze.c`).
//!
//! The forward-pass `costs` array is C `float` (`f32`). The cost model returns
//! `f64`. Storing a cost rounds to `f32`; comparing promotes that `f32` back to
//! `f64`.

use crate::consts::{LARGE_FLOAT, MAX_MATCH, MIN_MATCH, NUM_D, NUM_LL, WINDOW_MASK, WINDOW_SIZE};
use crate::deflate::calculate_block_size;
use crate::hash::Hash;
use crate::lz77::{find_longest_match, lz77_greedy, BlockState, Lz77Store};
use crate::symbols::{dist_extra_bits, dist_symbol, length_extra_bits, length_symbol};
use crate::tree::calculate_entropy;

#[derive(Clone)]
struct SymbolStats {
    litlens: [usize; NUM_LL],
    dists: [usize; NUM_D],
    ll_symbols: [f64; NUM_LL],
    d_symbols: [f64; NUM_D],
}

impl SymbolStats {
    fn new() -> Self {
        Self {
            litlens: [0; NUM_LL],
            dists: [0; NUM_D],
            ll_symbols: [0.0; NUM_LL],
            d_symbols: [0.0; NUM_D],
        }
    }
}

fn add_weighed_stat_freqs(
    stats1: &SymbolStats,
    w1: f64,
    stats2: &SymbolStats,
    w2: f64,
    result: &mut SymbolStats,
) {
    for i in 0..NUM_LL {
        let mixed = stats1.litlens[i] as f64 * w1 + stats2.litlens[i] as f64 * w2;
        result.litlens[i] = mixed as usize;
    }
    for i in 0..NUM_D {
        let mixed = stats1.dists[i] as f64 * w1 + stats2.dists[i] as f64 * w2;
        result.dists[i] = mixed as usize;
    }
    result.litlens[256] = 1;
}

struct RanState {
    m_w: u32,
    m_z: u32,
}

fn ran(state: &mut RanState) -> u32 {
    state.m_z = 36969u32
        .wrapping_mul(state.m_z & 65535)
        .wrapping_add(state.m_z >> 16);
    state.m_w = 18000u32
        .wrapping_mul(state.m_w & 65535)
        .wrapping_add(state.m_w >> 16);
    state.m_z.wrapping_shl(16).wrapping_add(state.m_w)
}

fn randomize_freqs(state: &mut RanState, freqs: &mut [usize], n: usize) {
    for i in 0..n {
        if (ran(state) >> 4) % 3 == 0 {
            let j = (ran(state) % (n as u32)) as usize;
            freqs[i] = freqs[j];
        }
    }
}

fn randomize_stat_freqs(state: &mut RanState, stats: &mut SymbolStats) {
    randomize_freqs(state, &mut stats.litlens, NUM_LL);
    randomize_freqs(state, &mut stats.dists, NUM_D);
    stats.litlens[256] = 1;
}

fn clear_stat_freqs(stats: &mut SymbolStats) {
    stats.litlens = [0; NUM_LL];
    stats.dists = [0; NUM_D];
}

fn get_cost_fixed(litlen: u32, dist: u32) -> f64 {
    if dist == 0 {
        if litlen <= 143 {
            8.0
        } else {
            9.0
        }
    } else {
        let dbits = dist_extra_bits(dist as i32);
        let lbits = length_extra_bits(litlen as i32);
        let lsym = length_symbol(litlen as i32);
        let mut cost = 0i32;
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
        let lsym = length_symbol(litlen as i32);
        let lbits = length_extra_bits(litlen as i32);
        let dsym = dist_symbol(dist as i32);
        let dbits = dist_extra_bits(dist as i32);
        f64::from(lbits + dbits) + stats.ll_symbols[lsym as usize] + stats.d_symbols[dsym as usize]
    }
}

fn get_cost_model_min_cost<F>(costmodel: &F) -> f64
where
    F: Fn(u32, u32) -> f64,
{
    // First distance of each deflate distance symbol (RFC 1951).
    const DSYMBOLS: [u32; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];

    let mut mincost = LARGE_FLOAT;
    let mut bestlength = 0u32;
    let mut length = 3u32;
    while length < 259 {
        let c = costmodel(length, 1);
        if c < mincost {
            bestlength = length;
            mincost = c;
        }
        length += 1;
    }

    mincost = LARGE_FLOAT;
    let mut bestdist = 0u32;
    for &dist in &DSYMBOLS {
        let c = costmodel(3, dist);
        if c < mincost {
            bestdist = dist;
            mincost = c;
        }
    }

    costmodel(bestlength, bestdist)
}

fn window_start(instart: usize) -> usize {
    if instart > WINDOW_SIZE {
        instart - WINDOW_SIZE
    } else {
        0
    }
}

fn warmup_hash_to(h: &mut Hash, in_data: &[u8], instart: usize, inend: usize) {
    let start = window_start(instart);
    h.reset(WINDOW_SIZE);
    h.warmup(in_data, start, inend);
    let mut i = start;
    while i < instart {
        h.update(in_data, i, inend);
        i += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn shortcut_long_repetition<F>(
    h: &mut Hash,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    i: &mut usize,
    j: &mut usize,
    costs: &mut [f32],
    length_array: &mut [u16],
    costmodel: &F,
) where
    F: Fn(u32, u32) -> f64,
{
    let pos = *i;
    if h.same[pos & WINDOW_MASK] <= (MAX_MATCH * 2) as u16
        || pos <= instart + MAX_MATCH + 1
        || pos + MAX_MATCH * 2 + 1 >= inend
        || h.same[(pos - MAX_MATCH) & WINDOW_MASK] <= MAX_MATCH as u16
    {
        return;
    }
    let symbolcost = costmodel(MAX_MATCH as u32, 1);
    // The outer scan increments `i` once more after this returns.
    for _ in 0..MAX_MATCH {
        let idx = *j + MAX_MATCH;
        costs[idx] = (costs[*j] as f64 + symbolcost) as f32;
        length_array[idx] = MAX_MATCH as u16;
        *i += 1;
        *j += 1;
        h.update(in_data, *i, inend);
    }
}

fn consider_length_costs<F>(
    costs: &mut [f32],
    length_array: &mut [u16],
    j: usize,
    kend: usize,
    mincost_add_costj: f64,
    sublen: &[u16],
    costmodel: &F,
) where
    F: Fn(u32, u32) -> f64,
{
    let mut k = 3usize;
    while k <= kend {
        if k >= sublen.len() {
            break;
        }
        let idx = j + k;
        if idx >= costs.len() || idx >= length_array.len() {
            break;
        }
        if (costs[idx] as f64) <= mincost_add_costj {
            k += 1;
            continue;
        }
        let new_cost = costmodel(k as u32, u32::from(sublen[k])) + (costs[j] as f64);
        if new_cost < (costs[idx] as f64) {
            costs[idx] = new_cost as f32;
            length_array[idx] = k as u16;
        }
        k += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn get_best_lengths<F>(
    s: &mut BlockState,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    costmodel: &F,
    length_array: &mut [u16],
    h: &mut Hash,
    costs: &mut [f32],
) -> f64
where
    F: Fn(u32, u32) -> f64,
{
    if instart == inend {
        return 0.0;
    }

    let blocksize = inend - instart;
    let mincost = get_cost_model_min_cost(costmodel);
    let large = LARGE_FLOAT as f32;
    for slot in &mut costs[1..=blocksize] {
        *slot = large;
    }
    costs[0] = 0.0;
    length_array[0] = 0;

    warmup_hash_to(h, in_data, instart, inend);

    let mut sublen = [0u16; MAX_MATCH + 1];
    let mut i = instart;
    while i < inend {
        let mut j = i - instart;
        h.update(in_data, i, inend);
        shortcut_long_repetition(
            h,
            in_data,
            instart,
            inend,
            &mut i,
            &mut j,
            costs,
            length_array,
            costmodel,
        );

        let mut dist = 0u16;
        let mut leng = 0u16;
        find_longest_match(
            s,
            h,
            in_data,
            i,
            inend,
            MAX_MATCH,
            Some(&mut sublen),
            &mut dist,
            &mut leng,
        );
        let _ = dist;

        // C checks `i + 1 <= inend`, which is the loop condition `i < inend`.
        if let Some(byte) = in_data.get(i) {
            let new_cost = costmodel(u32::from(*byte), 0) + (costs[j] as f64);
            if new_cost < (costs[j + 1] as f64) {
                costs[j + 1] = new_cost as f32;
                length_array[j + 1] = 1;
            }
        }

        let kend = usize::from(leng).min(inend - i);
        let mincost_add_costj = mincost + (costs[j] as f64);
        consider_length_costs(
            costs,
            length_array,
            j,
            kend,
            mincost_add_costj,
            &sublen,
            costmodel,
        );

        i += 1;
    }

    costs[blocksize] as f64
}

fn trace_backwards(length_array: &[u16], size: usize) -> Vec<u16> {
    let mut path = Vec::new();
    if size == 0 {
        return path;
    }
    let mut index = size;
    // A zero entry would not advance in C. Stop instead of looping forever.
    for _ in 0..=size {
        if index == 0 || index >= length_array.len() {
            break;
        }
        let len = length_array[index];
        if len == 0 {
            break;
        }
        let step = usize::from(len);
        if step > index {
            break;
        }
        path.push(len);
        index -= step;
    }
    path.reverse();
    path
}

fn follow_path(
    s: &mut BlockState,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    path: &[u16],
    store: &mut Lz77Store,
    h: &mut Hash,
) {
    if instart == inend {
        return;
    }

    warmup_hash_to(h, in_data, instart, inend);

    let mut pos = instart;
    for &path_len in path {
        if pos >= inend {
            break;
        }
        let mut length = path_len;
        h.update(in_data, pos, inend);

        if length >= MIN_MATCH as u16 {
            // `sublen` is null in C. The stored length is the path length;
            // `dummy_length` is only the matcher's report.
            let mut dist = 0u16;
            let mut dummy_length = 0u16;
            find_longest_match(
                s,
                h,
                in_data,
                pos,
                inend,
                usize::from(length),
                None,
                &mut dist,
                &mut dummy_length,
            );
            let _ = dummy_length;
            store.store_lit_len_dist(length, dist, pos);
        } else {
            length = 1;
            let Some(byte) = in_data.get(pos) else {
                break;
            };
            store.store_lit_len_dist(u16::from(*byte), 0, pos);
        }

        let mut j = 1usize;
        while j < usize::from(length) {
            h.update(in_data, pos + j, inend);
            j += 1;
        }
        pos += usize::from(length);
    }
}

#[allow(clippy::too_many_arguments)]
fn lz77_optimal_run<F>(
    s: &mut BlockState,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    length_array: &mut [u16],
    costmodel: &F,
    store: &mut Lz77Store,
    h: &mut Hash,
    costs: &mut [f32],
) -> f64
where
    F: Fn(u32, u32) -> f64,
{
    let cost = get_best_lengths(
        s,
        in_data,
        instart,
        inend,
        costmodel,
        length_array,
        h,
        costs,
    );
    let path = trace_backwards(length_array, inend - instart);
    follow_path(s, in_data, instart, inend, &path, store, h);
    cost
}

fn calculate_statistics(stats: &mut SymbolStats) {
    calculate_entropy(&stats.litlens, NUM_LL, &mut stats.ll_symbols);
    calculate_entropy(&stats.dists, NUM_D, &mut stats.d_symbols);
}

fn get_statistics(store: &Lz77Store, stats: &mut SymbolStats) {
    let n = store.size();
    for i in 0..n {
        if store.dists[i] == 0 {
            stats.litlens[usize::from(store.litlens[i])] += 1;
        } else {
            let lsym = length_symbol(i32::from(store.litlens[i])) as usize;
            let dsym = dist_symbol(i32::from(store.dists[i])) as usize;
            stats.litlens[lsym] += 1;
            stats.dists[dsym] += 1;
        }
    }
    stats.litlens[256] = 1;
    calculate_statistics(stats);
}

/// Optimal LZ77 for a dynamic tree, iterated `numiterations` times.
///
/// Stderr iteration logs are omitted. They do not affect the store.
pub fn lz77_optimal(
    s: &mut BlockState,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    numiterations: i32,
    store: &mut Lz77Store,
) {
    if inend < instart {
        return;
    }
    let blocksize = inend - instart;
    let mut length_array = vec![0u16; blocksize + 1];
    let mut costs = vec![0f32; blocksize + 1];
    let mut stats = SymbolStats::new();
    let mut beststats = SymbolStats::new();
    let mut laststats;
    let mut ran_state = RanState { m_w: 1, m_z: 2 };
    let mut lastrandomstep = -1i32;
    let mut bestcost = LARGE_FLOAT;
    let mut lastcost = 0.0;
    let mut hash = Hash::new(WINDOW_SIZE);

    let mut currentstore = Lz77Store::new();
    lz77_greedy(s, in_data, instart, inend, &mut currentstore, &mut hash);
    get_statistics(&currentstore, &mut stats);

    let mut i = 0i32;
    while i < numiterations {
        currentstore = Lz77Store::new();
        let _model_cost = lz77_optimal_run(
            s,
            in_data,
            instart,
            inend,
            &mut length_array,
            &|litlen, dist| get_cost_stat(litlen, dist, &stats),
            &mut currentstore,
            &mut hash,
            &mut costs,
        );
        let cost = calculate_block_size(&currentstore, 0, currentstore.size(), 2);
        if cost < bestcost {
            store.copy_from(&currentstore);
            beststats = stats.clone();
            bestcost = cost;
        }
        laststats = stats.clone();
        clear_stat_freqs(&mut stats);
        get_statistics(&currentstore, &mut stats);
        if lastrandomstep != -1 {
            let fresh = stats.clone();
            add_weighed_stat_freqs(&fresh, 1.0, &laststats, 0.5, &mut stats);
            calculate_statistics(&mut stats);
        }
        if i > 5 && cost == lastcost {
            stats = beststats.clone();
            randomize_stat_freqs(&mut ran_state, &mut stats);
            calculate_statistics(&mut stats);
            lastrandomstep = i;
        }
        lastcost = cost;
        i += 1;
    }
}

/// Optimal LZ77 for the fixed deflate tree. One shortest-path run.
pub fn lz77_optimal_fixed(
    s: &mut BlockState,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    store: &mut Lz77Store,
) {
    if inend < instart {
        return;
    }
    let blocksize = inend - instart;
    let mut length_array = vec![0u16; blocksize + 1];
    let mut costs = vec![0f32; blocksize + 1];
    let mut hash = Hash::new(WINDOW_SIZE);

    s.blockstart = instart;
    s.blockend = inend;

    let _model_cost = lz77_optimal_run(
        s,
        in_data,
        instart,
        inend,
        &mut length_array,
        &get_cost_fixed,
        store,
        &mut hash,
        &mut costs,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiply_with_carry_matches_c() {
        let mut state = RanState { m_w: 1, m_z: 2 };
        let mut got = [0u32; 8];
        for slot in &mut got {
            *slot = ran(&mut state);
        }
        assert_eq!(
            got,
            [
                550651472, 2842876160, 2457330511, 338550345, 2305076030, 3880432749, 263216920,
                3187702030,
            ]
        );
    }

    #[test]
    fn randomize_freqs_matches_c() {
        let mut state = RanState { m_w: 1, m_z: 2 };
        let mut freqs = [0usize, 1, 2, 3, 4, 5, 6, 7];
        randomize_freqs(&mut state, &mut freqs, 8);
        assert_eq!(freqs, [0, 1, 2, 6, 4, 5, 6, 7]);
    }

    #[test]
    fn weighed_freqs_truncate_toward_zero() {
        let mut a = SymbolStats::new();
        let mut b = SymbolStats::new();
        a.litlens[0] = 3;
        b.litlens[0] = 5;
        a.litlens[1] = 1;
        b.litlens[1] = 1;
        a.dists[0] = 7;
        b.dists[0] = 9;
        let mut out = SymbolStats::new();
        add_weighed_stat_freqs(&a, 1.0, &b, 0.5, &mut out);
        assert_eq!(out.litlens[0], 5);
        assert_eq!(out.litlens[1], 1);
        assert_eq!(out.dists[0], 11);
        assert_eq!(out.litlens[256], 1);
    }

    #[test]
    fn large_float_stored_as_f32() {
        let large = LARGE_FLOAT as f32;
        assert_eq!(large.to_bits(), 0x7149_f2ca);
        assert_eq!((large as f64).to_bits(), 0x4629_3e59_4000_0000);
        let sum = (large as f64 + 20.0) as f32;
        assert_eq!(sum.to_bits(), large.to_bits());
    }
}
