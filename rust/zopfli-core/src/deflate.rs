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

//! Block-size costing helpers from `src/zopfli/deflate.c` needed by squeeze.

use crate::lz77::Lz77Store;
use crate::symbols::{
    get_dist_symbol, get_dist_symbol_extra_bits, get_length_symbol, get_length_symbol_extra_bits,
};
use crate::tree::calculate_bit_lengths;
use crate::util::{ZOPFLI_NUM_D, ZOPFLI_NUM_LL};

fn patch_distance_codes_for_buggy_decoders(d_lengths: &mut [u32]) {
    let mut num_dist_codes = 0;
    for &len in d_lengths.iter().take(30) {
        if len != 0 {
            num_dist_codes += 1;
        }
        if num_dist_codes >= 2 {
            return;
        }
    }
    if num_dist_codes == 0 {
        d_lengths[0] = 1;
        d_lengths[1] = 1;
    } else if num_dist_codes == 1 {
        d_lengths[if d_lengths[0] != 0 { 1 } else { 0 }] = 1;
    }
}

fn encode_tree_size_only(
    ll_lengths: &[u32],
    d_lengths: &[u32],
    use_16: bool,
    use_17: bool,
    use_18: bool,
) -> usize {
    let mut hlit = 29usize;
    let mut hdist = 29usize;
    let mut clcounts = [0usize; 19];
    let order = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];

    while hlit > 0 && ll_lengths[257 + hlit - 1] == 0 {
        hlit -= 1;
    }
    while hdist > 0 && d_lengths[1 + hdist - 1] == 0 {
        hdist -= 1;
    }
    let hlit2 = hlit + 257;
    let lld_total = hlit2 + hdist + 1;

    let mut i = 0;
    while i < lld_total {
        let symbol = if i < hlit2 {
            ll_lengths[i]
        } else {
            d_lengths[i - hlit2]
        } as u8;
        let mut count = 1usize;
        if use_16 || (symbol == 0 && (use_17 || use_18)) {
            let mut j = i + 1;
            while j < lld_total {
                let other = if j < hlit2 {
                    ll_lengths[j]
                } else {
                    d_lengths[j - hlit2]
                } as u8;
                if symbol != other {
                    break;
                }
                count += 1;
                j += 1;
            }
        }
        i += count - 1;

        if symbol == 0 && count >= 3 {
            if use_18 {
                while count >= 11 {
                    let count2 = count.min(138);
                    clcounts[18] += 1;
                    count -= count2;
                }
            }
            if use_17 {
                while count >= 3 {
                    let count2 = count.min(10);
                    clcounts[17] += 1;
                    count -= count2;
                }
            }
        }

        if use_16 && count >= 4 {
            count -= 1;
            clcounts[symbol as usize] += 1;
            while count >= 3 {
                let count2 = count.min(6);
                clcounts[16] += 1;
                count -= count2;
            }
        }

        clcounts[symbol as usize] += count;
        i += 1;
    }

    let mut clcl = [0u32; 19];
    calculate_bit_lengths(&clcounts, 7, &mut clcl);

    let mut hclen = 15usize;
    while hclen > 0 && clcounts[order[hclen + 4 - 1]] == 0 {
        hclen -= 1;
    }

    let mut result_size = 14;
    result_size += (hclen + 4) * 3;
    for i in 0..19 {
        result_size += clcl[i] as usize * clcounts[i];
    }
    result_size += clcounts[16] * 2;
    result_size += clcounts[17] * 3;
    result_size += clcounts[18] * 7;
    result_size
}

fn calculate_tree_size(ll_lengths: &[u32], d_lengths: &[u32]) -> usize {
    let mut result = 0usize;
    for i in 0..8 {
        let size = encode_tree_size_only(ll_lengths, d_lengths, i & 1 != 0, i & 2 != 0, i & 4 != 0);
        if result == 0 || size < result {
            result = size;
        }
    }
    result
}

fn get_fixed_tree(ll_lengths: &mut [u32], d_lengths: &mut [u32]) {
    ll_lengths[..144].fill(8);
    ll_lengths[144..256].fill(9);
    ll_lengths[256..280].fill(7);
    ll_lengths[280..288].fill(8);
    d_lengths[..32].fill(5);
}

fn calculate_block_symbol_size_small(
    ll_lengths: &[u32],
    d_lengths: &[u32],
    lz77: &Lz77Store,
    lstart: usize,
    lend: usize,
) -> usize {
    let mut result = 0usize;
    for i in lstart..lend {
        debug_assert!(i < lz77.size());
        debug_assert!(lz77.litlens[i] < 259);
        if lz77.dists[i] == 0 {
            result += ll_lengths[lz77.litlens[i] as usize] as usize;
        } else {
            let ll_symbol = get_length_symbol(i32::from(lz77.litlens[i]));
            let d_symbol = get_dist_symbol(i32::from(lz77.dists[i]));
            result += ll_lengths[ll_symbol as usize] as usize;
            result += d_lengths[d_symbol as usize] as usize;
            result += get_length_symbol_extra_bits(ll_symbol) as usize;
            result += get_dist_symbol_extra_bits(d_symbol) as usize;
        }
    }
    result += ll_lengths[256] as usize;
    result
}

fn calculate_block_symbol_size_given_counts(
    ll_counts: &[usize],
    d_counts: &[usize],
    ll_lengths: &[u32],
    d_lengths: &[u32],
    lz77: &Lz77Store,
    lstart: usize,
    lend: usize,
) -> usize {
    if lstart + ZOPFLI_NUM_LL * 3 > lend {
        return calculate_block_symbol_size_small(ll_lengths, d_lengths, lz77, lstart, lend);
    }
    let mut result = 0usize;
    for i in 0..256 {
        result += ll_lengths[i] as usize * ll_counts[i];
    }
    for i in 257..286 {
        result += ll_lengths[i] as usize * ll_counts[i];
        result += get_length_symbol_extra_bits(i as i32) as usize * ll_counts[i];
    }
    for i in 0..30 {
        result += d_lengths[i] as usize * d_counts[i];
        result += get_dist_symbol_extra_bits(i as i32) as usize * d_counts[i];
    }
    result += ll_lengths[256] as usize;
    result
}

fn calculate_block_symbol_size(
    ll_lengths: &[u32],
    d_lengths: &[u32],
    lz77: &Lz77Store,
    lstart: usize,
    lend: usize,
) -> usize {
    if lstart + ZOPFLI_NUM_LL * 3 > lend {
        calculate_block_symbol_size_small(ll_lengths, d_lengths, lz77, lstart, lend)
    } else {
        let mut ll_counts = [0usize; ZOPFLI_NUM_LL];
        let mut d_counts = [0usize; ZOPFLI_NUM_D];
        lz77.get_histogram(lstart, lend, &mut ll_counts, &mut d_counts);
        calculate_block_symbol_size_given_counts(
            &ll_counts, &d_counts, ll_lengths, d_lengths, lz77, lstart, lend,
        )
    }
}

fn abs_diff(x: usize, y: usize) -> usize {
    x.abs_diff(y)
}

/// Matching C `OptimizeHuffmanForRle`.
fn optimize_huffman_for_rle(mut length: i32, counts: &mut [usize]) {
    loop {
        if length == 0 {
            return;
        }
        if counts[(length - 1) as usize] != 0 {
            break;
        }
        length -= 1;
    }

    let length = length as usize;
    let mut good_for_rle = vec![0i32; length];

    let mut symbol = counts[0];
    let mut stride = 0usize;
    for i in 0..=length {
        if i == length || counts[i] != symbol {
            if (symbol == 0 && stride >= 5) || (symbol != 0 && stride >= 7) {
                for k in 0..stride {
                    good_for_rle[i - k - 1] = 1;
                }
            }
            stride = 1;
            if i != length {
                symbol = counts[i];
            }
        } else {
            stride += 1;
        }
    }

    stride = 0;
    let mut limit = counts[0];
    let mut sum = 0usize;
    for i in 0..=length {
        if i == length || good_for_rle[i] != 0 || abs_diff(counts[i], limit) >= 4 {
            if stride >= 4 || (stride >= 3 && sum == 0) {
                let mut count = (sum + stride / 2) / stride;
                if count < 1 {
                    count = 1;
                }
                if sum == 0 {
                    count = 0;
                }
                for k in 0..stride {
                    counts[i - k - 1] = count;
                }
            }
            stride = 0;
            sum = 0;
            if i < length.saturating_sub(3) && i < length {
                // C: `if (i < length - 3)` with signed length.
                if (i as i32) < length as i32 - 3 {
                    limit = (counts[i] + counts[i + 1] + counts[i + 2] + counts[i + 3] + 2) / 4;
                } else if i < length {
                    limit = counts[i];
                } else {
                    limit = 0;
                }
            } else if i < length {
                limit = counts[i];
            } else {
                limit = 0;
            }
        }
        stride += 1;
        if i != length {
            sum += counts[i];
        }
    }
}

fn try_optimize_huffman_for_rle(
    lz77: &Lz77Store,
    lstart: usize,
    lend: usize,
    ll_counts: &[usize],
    d_counts: &[usize],
    ll_lengths: &mut [u32],
    d_lengths: &mut [u32],
) -> f64 {
    let treesize = calculate_tree_size(ll_lengths, d_lengths) as f64;
    let datasize = calculate_block_symbol_size_given_counts(
        ll_counts, d_counts, ll_lengths, d_lengths, lz77, lstart, lend,
    ) as f64;

    let mut ll_counts2 = [0usize; ZOPFLI_NUM_LL];
    let mut d_counts2 = [0usize; ZOPFLI_NUM_D];
    ll_counts2.copy_from_slice(ll_counts);
    d_counts2.copy_from_slice(d_counts);
    optimize_huffman_for_rle(ZOPFLI_NUM_LL as i32, &mut ll_counts2);
    optimize_huffman_for_rle(ZOPFLI_NUM_D as i32, &mut d_counts2);

    let mut ll_lengths2 = [0u32; ZOPFLI_NUM_LL];
    let mut d_lengths2 = [0u32; ZOPFLI_NUM_D];
    calculate_bit_lengths(&ll_counts2, 15, &mut ll_lengths2);
    calculate_bit_lengths(&d_counts2, 15, &mut d_lengths2);
    patch_distance_codes_for_buggy_decoders(&mut d_lengths2);

    let treesize2 = calculate_tree_size(&ll_lengths2, &d_lengths2) as f64;
    let datasize2 = calculate_block_symbol_size_given_counts(
        ll_counts,
        d_counts,
        &ll_lengths2,
        &d_lengths2,
        lz77,
        lstart,
        lend,
    ) as f64;

    if treesize2 + datasize2 < treesize + datasize {
        ll_lengths.copy_from_slice(&ll_lengths2);
        d_lengths.copy_from_slice(&d_lengths2);
        treesize2 + datasize2
    } else {
        treesize + datasize
    }
}

fn get_dynamic_lengths(
    lz77: &Lz77Store,
    lstart: usize,
    lend: usize,
    ll_lengths: &mut [u32],
    d_lengths: &mut [u32],
) -> f64 {
    let mut ll_counts = [0usize; ZOPFLI_NUM_LL];
    let mut d_counts = [0usize; ZOPFLI_NUM_D];
    lz77.get_histogram(lstart, lend, &mut ll_counts, &mut d_counts);
    ll_counts[256] = 1;
    calculate_bit_lengths(&ll_counts, 15, ll_lengths);
    calculate_bit_lengths(&d_counts, 15, d_lengths);
    patch_distance_codes_for_buggy_decoders(d_lengths);
    try_optimize_huffman_for_rle(
        lz77, lstart, lend, &ll_counts, &d_counts, ll_lengths, d_lengths,
    )
}

/// Calculates block size in bits, matching `ZopfliCalculateBlockSize`.
pub fn calculate_block_size(lz77: &Lz77Store, lstart: usize, lend: usize, btype: i32) -> f64 {
    let mut ll_lengths = [0u32; ZOPFLI_NUM_LL];
    let mut d_lengths = [0u32; ZOPFLI_NUM_D];
    let mut result = 3.0;

    if btype == 0 {
        let length = lz77.get_byte_range(lstart, lend);
        let rem = length % 65535;
        let blocks = length / 65535 + usize::from(rem != 0);
        return (blocks * 5 * 8 + length * 8) as f64;
    }
    if btype == 1 {
        get_fixed_tree(&mut ll_lengths, &mut d_lengths);
        result += calculate_block_symbol_size(&ll_lengths, &d_lengths, lz77, lstart, lend) as f64;
    } else {
        result += get_dynamic_lengths(lz77, lstart, lend, &mut ll_lengths, &mut d_lengths);
    }
    result
}
