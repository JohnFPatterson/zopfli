//! Block boundary selection (`src/zopfli/blocksplitter.c`).
//!
//! Split-point vectors are C's dynamic arrays: the block count is `len + 1`,
//! and the uncompressed or LZ77 end is not stored.

use crate::consts::{LARGE_FLOAT, WINDOW_SIZE};
use crate::deflate::calculate_block_size_auto_type;
use crate::hash::Hash;
use crate::lz77::{lz77_greedy, BlockState, Lz77Store};
use crate::Options;

fn find_minimum<F>(mut f: F, mut start: usize, mut end: usize) -> (usize, f64)
where
    F: FnMut(usize) -> f64,
{
    if end.wrapping_sub(start) < 1024 {
        let mut best = LARGE_FLOAT;
        let mut result = start;
        let mut i = start;
        while i < end {
            let v = f(i);
            if v < best {
                best = v;
                result = i;
            }
            i = i.wrapping_add(1);
        }
        (result, best)
    } else {
        const NUM: usize = 9;
        let mut lastbest = LARGE_FLOAT;
        let mut pos = start;
        loop {
            if end.wrapping_sub(start) <= NUM {
                break;
            }
            let mut p = [0usize; NUM];
            let mut vp = [0.0f64; NUM];
            let step = end.wrapping_sub(start) / (NUM + 1);
            for i in 0..NUM {
                p[i] = start.wrapping_add((i + 1).wrapping_mul(step));
                vp[i] = f(p[i]);
            }
            let mut besti = 0usize;
            let mut best = vp[0];
            for (i, &sample) in vp.iter().enumerate().skip(1) {
                if sample < best {
                    best = sample;
                    besti = i;
                }
            }
            if best > lastbest {
                break;
            }
            start = if besti == 0 { start } else { p[besti - 1] };
            end = if besti == NUM - 1 { end } else { p[besti + 1] };
            pos = p[besti];
            lastbest = best;
        }
        (pos, lastbest)
    }
}

fn estimate_cost(lz77: &Lz77Store, lstart: usize, lend: usize) -> f64 {
    calculate_block_size_auto_type(lz77, lstart, lend)
}

fn add_sorted(value: usize, out: &mut Vec<usize>) {
    out.push(value);
    let outsize = out.len();
    let mut i = 0usize;
    while i + 1 < outsize {
        if out[i] > value {
            let mut j = outsize - 1;
            while j > i {
                out[j] = out[j - 1];
                j -= 1;
            }
            out[i] = value;
            break;
        }
        i += 1;
    }
}

fn find_largest_splittable_block(
    lz77size: usize,
    done: &[u8],
    splitpoints: &[usize],
    npoints: usize,
) -> Option<(usize, usize)> {
    let mut longest = 0usize;
    let mut found = None;
    for i in 0..=npoints {
        let start = if i == 0 { 0 } else { splitpoints[i - 1] };
        // The last block's exclusive end is `lz77size - 1`, not `lz77size`.
        let end = if i == npoints {
            lz77size.wrapping_sub(1)
        } else {
            splitpoints[i]
        };
        let span = end.wrapping_sub(start);
        let blocked = !matches!(done.get(start), Some(0));
        if !blocked && span > longest {
            found = Some((start, end));
            longest = span;
        }
    }
    found
}

/// Split points are indices into `lz77`. `maxblocks == 0` means no limit.
///
/// Stderr printing of the points is omitted.
pub fn block_split_lz77(options: &Options, lz77: &Lz77Store, maxblocks: usize) -> Vec<usize> {
    let _ = options.verbose;
    if lz77.size() < 10 {
        return Vec::new();
    }

    let mut done = vec![0u8; lz77.size()];
    let mut splitpoints = Vec::new();
    let mut lstart = 0usize;
    let mut lend = lz77.size();
    let mut numblocks = 1usize;

    loop {
        if maxblocks > 0 && numblocks >= maxblocks {
            break;
        }

        let start = lstart;
        let end = lend;
        let (llpos, splitcost) = find_minimum(
            |i| estimate_cost(lz77, start, i) + estimate_cost(lz77, i, end),
            start + 1,
            end,
        );
        let origcost = estimate_cost(lz77, lstart, lend);
        if splitcost > origcost || llpos == lstart + 1 || llpos == lend {
            if let Some(slot) = done.get_mut(lstart) {
                *slot = 1;
            }
        } else {
            add_sorted(llpos, &mut splitpoints);
            numblocks += 1;
        }

        match find_largest_splittable_block(lz77.size(), &done, &splitpoints, splitpoints.len()) {
            Some((next_start, next_end)) => {
                lstart = next_start;
                lend = next_end;
            }
            None => break,
        }
        if lend.wrapping_sub(lstart) < 10 {
            break;
        }
    }

    splitpoints
}

/// Uncompressed split points. The LZ77 pass used to choose them is greedy.
pub fn block_split(
    options: &Options,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    maxblocks: usize,
) -> Vec<usize> {
    let mut store = Lz77Store::new();
    let mut state = BlockState::new(*options, instart, inend, false);
    let mut hash = Hash::new(WINDOW_SIZE);
    lz77_greedy(&mut state, in_data, instart, inend, &mut store, &mut hash);
    let lz77splitpoints = block_split_lz77(options, &store, maxblocks);
    let nlz77points = lz77splitpoints.len();

    let mut splitpoints = Vec::new();
    if nlz77points == 0 {
        return splitpoints;
    }

    let mut pos = instart;
    for i in 0..store.size() {
        let length = if store.dists[i] == 0 {
            1
        } else {
            usize::from(store.litlens[i])
        };
        if lz77splitpoints[splitpoints.len()] == i {
            splitpoints.push(pos);
            if splitpoints.len() == nlz77points {
                break;
            }
        }
        pos += length;
    }
    splitpoints
}

/// Equal-sized blocks. Includes `instart` and does not include `inend`.
pub fn block_split_simple(instart: usize, inend: usize, blocksize: usize) -> Vec<usize> {
    let mut splitpoints = Vec::new();
    let mut i = instart;
    while i < inend {
        splitpoints.push(i);
        i = i.wrapping_add(blocksize);
    }
    splitpoints
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parabola(i: usize, target: usize) -> f64 {
        let d = i as f64 - target as f64;
        d * d
    }

    #[test]
    fn find_minimum_matches_c() {
        let (idx, cost) = find_minimum(|i| parabola(i, 100), 0, 50);
        assert_eq!(idx, 49);
        assert_eq!(cost, 2601.0);

        let (idx, cost) = find_minimum(|i| parabola(i, 5000), 0, 20000);
        assert_eq!(idx, 5000);
        assert_eq!(cost, 0.0);

        // Sampled search stops on the collapsed window; C returns 9 here, not 10.
        let (idx, cost) = find_minimum(|i| parabola(i, 10), 0, 20000);
        assert_eq!(idx, 9);
        assert_eq!(cost, 1.0);
    }

    #[test]
    fn add_sorted_matches_c() {
        let mut out = Vec::new();
        for value in [5usize, 1, 4, 4, 9, 2] {
            add_sorted(value, &mut out);
        }
        assert_eq!(out, [1, 2, 4, 4, 5, 9]);
    }

    #[test]
    fn simple_split_includes_start_not_end() {
        assert_eq!(block_split_simple(10, 25, 5), vec![10, 15, 20]);
        assert_eq!(block_split_simple(5, 5, 8), Vec::<usize>::new());
        assert_eq!(block_split_simple(0, 1, 100), vec![0]);
    }
}
