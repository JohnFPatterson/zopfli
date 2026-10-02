//! DEFLATE block encoder.
//!
//! Huffman symbols are packed most-significant bit first. Headers, extra
//! length and distance bits, and the dynamic-tree description are packed
//! least-significant bit first. A stored block writes its three header bits,
//! then sets the bit pointer to 0 and appends LEN, NLEN, and the payload as
//! whole bytes, leaving unused high bits in the header byte as zero.

use crate::blocksplitter::{block_split, block_split_lz77};
use crate::consts::{MASTER_BLOCK_SIZE, NUM_D, NUM_LL};
use crate::lz77::{BlockState, Lz77Store};
use crate::squeeze::{lz77_optimal, lz77_optimal_fixed};
use crate::symbols::{
    get_dist_extra_bits, get_dist_extra_bits_value, get_dist_symbol, get_dist_symbol_extra_bits,
    get_length_extra_bits, get_length_extra_bits_value, get_length_symbol,
    get_length_symbol_extra_bits,
};
use crate::tree::{calculate_bit_lengths, lengths_to_symbols};
use crate::Options;

/// Code-length code order from the DEFLATE specification.
const ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Append one bit, least-significant side first.
///
/// When `bp` is 0 a fresh zero byte is appended before the bit is stored.
fn add_bit(bit: u32, bp: &mut u8, out: &mut Vec<u8>) {
    if *bp == 0 {
        out.push(0);
    }
    let last = out.len() - 1;
    out[last] |= (bit << u32::from(*bp)) as u8;
    *bp = (*bp + 1) & 7;
}

/// Append `length` low bits of `symbol`, least-significant bit first.
fn add_bits(symbol: u32, length: u32, bp: &mut u8, out: &mut Vec<u8>) {
    for i in 0..length {
        add_bit((symbol >> i) & 1, bp, out);
    }
}

/// Append `length` low bits of `symbol`, most-significant bit first.
fn add_huffman_bits(symbol: u32, length: u32, bp: &mut u8, out: &mut Vec<u8>) {
    for i in 0..length {
        let bit = (symbol >> (length - i - 1)) & 1;
        add_bit(bit, bp, out);
    }
}

/// Ensure at least two non-zero distance codes for buggy decoders.
fn patch_distance_codes_for_buggy_decoders(d_lengths: &mut [u32]) {
    let mut num_dist_codes = 0i32;
    for i in 0..30 {
        if d_lengths[i] != 0 {
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
        let index = if d_lengths[0] != 0 { 1 } else { 0 };
        d_lengths[index] = 1;
    }
}

fn length_at(ll_lengths: &[u32], d_lengths: &[u32], hlit2: usize, index: usize) -> u32 {
    if index < hlit2 {
        ll_lengths[index]
    } else {
        d_lengths[index - hlit2]
    }
}

/// Encode the dynamic Huffman trees.
///
/// Returns the encoded size in bits. When `size_only` is set, nothing is written.
fn encode_tree(
    ll_lengths: &[u32],
    d_lengths: &[u32],
    use_16: bool,
    use_17: bool,
    use_18: bool,
    bp: &mut u8,
    out: &mut Vec<u8>,
    size_only: bool,
) -> usize {
    let mut hlit = 29usize;
    let mut hdist = 29usize;
    while hlit > 0 && ll_lengths[257 + hlit - 1] == 0 {
        hlit -= 1;
    }
    while hdist > 0 && d_lengths[hdist] == 0 {
        hdist -= 1;
    }
    let hlit2 = hlit + 257;
    let lld_total = hlit2 + hdist + 1;

    let mut clcounts = [0usize; 19];
    let mut rle: Vec<u32> = Vec::new();
    let mut rle_bits: Vec<u32> = Vec::new();

    let mut i = 0usize;
    while i < lld_total {
        let symbol = length_at(ll_lengths, d_lengths, hlit2, i);
        let mut count: u32 = 1;
        if use_16 || (symbol == 0 && (use_17 || use_18)) {
            let mut j = i + 1;
            while j < lld_total && symbol == length_at(ll_lengths, d_lengths, hlit2, j) {
                count += 1;
                j += 1;
            }
        }
        // The C loop also increments `i` once per iteration.
        i += count as usize;

        if symbol == 0 && count >= 3 {
            if use_18 {
                while count >= 11 {
                    let count2 = if count > 138 { 138 } else { count };
                    if !size_only {
                        rle.push(18);
                        rle_bits.push(count2 - 11);
                    }
                    clcounts[18] += 1;
                    count -= count2;
                }
            }
            if use_17 {
                while count >= 3 {
                    let count2 = if count > 10 { 10 } else { count };
                    if !size_only {
                        rle.push(17);
                        rle_bits.push(count2 - 3);
                    }
                    clcounts[17] += 1;
                    count -= count2;
                }
            }
        }

        if use_16 && count >= 4 {
            count -= 1;
            clcounts[symbol as usize] += 1;
            if !size_only {
                rle.push(symbol);
                rle_bits.push(0);
            }
            while count >= 3 {
                let count2 = if count > 6 { 6 } else { count };
                if !size_only {
                    rle.push(16);
                    rle_bits.push(count2 - 3);
                }
                clcounts[16] += 1;
                count -= count2;
            }
        }

        clcounts[symbol as usize] += count as usize;
        while count > 0 {
            if !size_only {
                rle.push(symbol);
                rle_bits.push(0);
            }
            count -= 1;
        }
    }

    let mut clcl = [0u32; 19];
    calculate_bit_lengths(&clcounts, 19, 7, &mut clcl);
    let mut clsymbols = [0u32; 19];
    if !size_only {
        lengths_to_symbols(&clcl, 19, 7, &mut clsymbols);
    }

    let mut hclen = 15usize;
    while hclen > 0 && clcounts[ORDER[hclen + 4 - 1]] == 0 {
        hclen -= 1;
    }

    if !size_only {
        add_bits(hlit as u32, 5, bp, out);
        add_bits(hdist as u32, 5, bp, out);
        add_bits(hclen as u32, 4, bp, out);
        for order_i in 0..(hclen + 4) {
            add_bits(clcl[ORDER[order_i]], 3, bp, out);
        }
        for rle_i in 0..rle.len() {
            let rle_symbol = rle[rle_i];
            let code = clsymbols[rle_symbol as usize];
            add_huffman_bits(code, clcl[rle_symbol as usize], bp, out);
            if rle_symbol == 16 {
                add_bits(rle_bits[rle_i], 2, bp, out);
            } else if rle_symbol == 17 {
                add_bits(rle_bits[rle_i], 3, bp, out);
            } else if rle_symbol == 18 {
                add_bits(rle_bits[rle_i], 7, bp, out);
            }
        }
    }

    let mut result_size = 14usize;
    result_size += (hclen + 4) * 3;
    for i_cl in 0..19 {
        result_size += clcl[i_cl] as usize * clcounts[i_cl];
    }
    result_size += clcounts[16] * 2;
    result_size += clcounts[17] * 3;
    result_size += clcounts[18] * 7;
    result_size
}

fn add_dynamic_tree(ll_lengths: &[u32], d_lengths: &[u32], bp: &mut u8, out: &mut Vec<u8>) {
    let mut best = 0i32;
    let mut bestsize = 0usize;
    for i in 0..8i32 {
        let size = encode_tree(
            ll_lengths,
            d_lengths,
            (i & 1) != 0,
            (i & 2) != 0,
            (i & 4) != 0,
            bp,
            out,
            true,
        );
        if bestsize == 0 || size < bestsize {
            bestsize = size;
            best = i;
        }
    }
    encode_tree(
        ll_lengths,
        d_lengths,
        (best & 1) != 0,
        (best & 2) != 0,
        (best & 4) != 0,
        bp,
        out,
        false,
    );
}

fn calculate_tree_size(ll_lengths: &[u32], d_lengths: &[u32]) -> usize {
    let mut result = 0usize;
    let mut dummy_bp = 0u8;
    let mut dummy_out = Vec::new();
    for i in 0..8i32 {
        let size = encode_tree(
            ll_lengths,
            d_lengths,
            (i & 1) != 0,
            (i & 2) != 0,
            (i & 4) != 0,
            &mut dummy_bp,
            &mut dummy_out,
            true,
        );
        if result == 0 || size < result {
            result = size;
        }
    }
    result
}

fn add_lz77_data(
    lz77: &Lz77Store,
    lstart: usize,
    lend: usize,
    ll_symbols: &[u32],
    ll_lengths: &[u32],
    d_symbols: &[u32],
    d_lengths: &[u32],
    bp: &mut u8,
    out: &mut Vec<u8>,
) {
    for i in lstart..lend {
        let dist = lz77.dists[i];
        let litlen = lz77.litlens[i];
        if dist == 0 {
            let lit = usize::from(litlen);
            add_huffman_bits(ll_symbols[lit], ll_lengths[lit], bp, out);
        } else {
            let lls = get_length_symbol(i32::from(litlen)) as usize;
            let ds = get_dist_symbol(i32::from(dist)) as usize;
            add_huffman_bits(ll_symbols[lls], ll_lengths[lls], bp, out);
            add_bits(
                get_length_extra_bits_value(i32::from(litlen)) as u32,
                get_length_extra_bits(i32::from(litlen)) as u32,
                bp,
                out,
            );
            add_huffman_bits(d_symbols[ds], d_lengths[ds], bp, out);
            add_bits(
                get_dist_extra_bits_value(i32::from(dist)) as u32,
                get_dist_extra_bits(i32::from(dist)) as u32,
                bp,
                out,
            );
        }
    }
}

fn get_fixed_tree(ll_lengths: &mut [u32], d_lengths: &mut [u32]) {
    for len in &mut ll_lengths[..144] {
        *len = 8;
    }
    for len in &mut ll_lengths[144..256] {
        *len = 9;
    }
    for len in &mut ll_lengths[256..280] {
        *len = 7;
    }
    for len in &mut ll_lengths[280..288] {
        *len = 8;
    }
    for len in &mut d_lengths[..32] {
        *len = 5;
    }
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
        if lz77.dists[i] == 0 {
            result += ll_lengths[usize::from(lz77.litlens[i])] as usize;
        } else {
            let ll_symbol = get_length_symbol(i32::from(lz77.litlens[i])) as usize;
            let d_symbol = get_dist_symbol(i32::from(lz77.dists[i])) as usize;
            result += ll_lengths[ll_symbol] as usize;
            result += d_lengths[d_symbol] as usize;
            result += get_length_symbol_extra_bits(ll_symbol as i32) as usize;
            result += get_dist_symbol_extra_bits(d_symbol as i32) as usize;
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
    if lstart + NUM_LL * 3 > lend {
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
    if lstart + NUM_LL * 3 > lend {
        calculate_block_symbol_size_small(ll_lengths, d_lengths, lz77, lstart, lend)
    } else {
        let mut ll_counts = [0usize; NUM_LL];
        let mut d_counts = [0usize; NUM_D];
        lz77.histogram(lstart, lend, &mut ll_counts, &mut d_counts);
        calculate_block_symbol_size_given_counts(
            &ll_counts, &d_counts, ll_lengths, d_lengths, lz77, lstart, lend,
        )
    }
}

/// Reshape histogram counts so the following Huffman RLE is more likely to win.
fn optimize_huffman_for_rle(mut length: i32, counts: &mut [usize]) {
    while length >= 0 {
        if length == 0 {
            return;
        }
        if counts[(length as usize) - 1] != 0 {
            break;
        }
        length -= 1;
    }

    let mut good_for_rle = vec![0i32; length as usize];
    let mut symbol = counts[0];
    let mut stride: i32 = 0;
    let mut i: i32 = 0;
    while i < length + 1 {
        if i == length || counts[i as usize] != symbol {
            if (symbol == 0 && stride >= 5) || (symbol != 0 && stride >= 7) {
                let mut k = 0i32;
                while k < stride {
                    good_for_rle[(i - k - 1) as usize] = 1;
                    k += 1;
                }
            }
            stride = 1;
            if i != length {
                symbol = counts[i as usize];
            }
        } else {
            stride += 1;
        }
        i += 1;
    }

    stride = 0;
    let mut limit = counts[0];
    let mut sum = 0usize;
    i = 0;
    while i < length + 1 {
        if i == length || good_for_rle[i as usize] != 0 || counts[i as usize].abs_diff(limit) >= 4 {
            if stride >= 4 || (stride >= 3 && sum == 0) {
                let stride_us = stride as usize;
                let mut count = ((sum + stride_us / 2) / stride_us) as i32;
                if count < 1 {
                    count = 1;
                }
                if sum == 0 {
                    count = 0;
                }
                let mut k = 0i32;
                while k < stride {
                    counts[(i - k - 1) as usize] = count as usize;
                    k += 1;
                }
            }
            stride = 0;
            sum = 0;
            if i < length - 3 {
                limit = (counts[i as usize]
                    + counts[(i + 1) as usize]
                    + counts[(i + 2) as usize]
                    + counts[(i + 3) as usize]
                    + 2)
                    / 4;
            } else if i < length {
                limit = counts[i as usize];
            } else {
                limit = 0;
            }
        }
        stride += 1;
        if i != length {
            sum += counts[i as usize];
        }
        i += 1;
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
    let mut ll_counts2 = [0usize; NUM_LL];
    let mut d_counts2 = [0usize; NUM_D];
    ll_counts2.copy_from_slice(ll_counts);
    d_counts2.copy_from_slice(d_counts);
    let mut ll_lengths2 = [0u32; NUM_LL];
    let mut d_lengths2 = [0u32; NUM_D];

    let treesize = calculate_tree_size(ll_lengths, d_lengths) as f64;
    let datasize = calculate_block_symbol_size_given_counts(
        ll_counts, d_counts, ll_lengths, d_lengths, lz77, lstart, lend,
    ) as f64;

    optimize_huffman_for_rle(NUM_LL as i32, &mut ll_counts2);
    optimize_huffman_for_rle(NUM_D as i32, &mut d_counts2);
    calculate_bit_lengths(&ll_counts2, NUM_LL, 15, &mut ll_lengths2);
    calculate_bit_lengths(&d_counts2, NUM_D, 15, &mut d_lengths2);
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
    let mut ll_counts = [0usize; NUM_LL];
    let mut d_counts = [0usize; NUM_D];
    lz77.histogram(lstart, lend, &mut ll_counts, &mut d_counts);
    ll_counts[256] = 1;
    calculate_bit_lengths(&ll_counts, NUM_LL, 15, ll_lengths);
    calculate_bit_lengths(&d_counts, NUM_D, 15, d_lengths);
    patch_distance_codes_for_buggy_decoders(d_lengths);
    try_optimize_huffman_for_rle(
        lz77, lstart, lend, &ll_counts, &d_counts, ll_lengths, d_lengths,
    )
}

/// Bit size of one DEFLATE block, including the 3-bit header for typed blocks.
///
/// For stored blocks (`btype == 0`) the whole expression is an integer count of
/// header and payload bytes converted to `f64` only at the return.
pub fn calculate_block_size(lz77: &Lz77Store, lstart: usize, lend: usize, btype: i32) -> f64 {
    let mut result = 3.0f64;
    if btype == 0 {
        let length = lz77.byte_range(lstart, lend);
        let rem = length % 65535;
        let blocks = length / 65535 + usize::from(rem != 0);
        return (blocks * 5 * 8 + length * 8) as f64;
    }
    let mut ll_lengths = [0u32; NUM_LL];
    let mut d_lengths = [0u32; NUM_D];
    if btype == 1 {
        get_fixed_tree(&mut ll_lengths, &mut d_lengths);
        result += calculate_block_symbol_size(&ll_lengths, &d_lengths, lz77, lstart, lend) as f64;
    } else {
        result += get_dynamic_lengths(lz77, lstart, lend, &mut ll_lengths, &mut d_lengths);
    }
    result
}

/// Smallest block size among stored, fixed, and dynamic encodings.
///
/// When the store holds more than 1000 symbols the fixed-tree cost is not
/// computed and the stored cost stands in for it.
pub fn calculate_block_size_auto_type(lz77: &Lz77Store, lstart: usize, lend: usize) -> f64 {
    let uncompressedcost = calculate_block_size(lz77, lstart, lend, 0);
    let fixedcost = if lz77.size() > 1000 {
        uncompressedcost
    } else {
        calculate_block_size(lz77, lstart, lend, 1)
    };
    let dyncost = calculate_block_size(lz77, lstart, lend, 2);
    if uncompressedcost < fixedcost && uncompressedcost < dyncost {
        uncompressedcost
    } else if fixedcost < dyncost {
        fixedcost
    } else {
        dyncost
    }
}

/// Emit one or more stored blocks covering `in_data[instart..inend]`.
fn add_non_compressed_block(
    in_data: &[u8],
    instart: usize,
    inend: usize,
    final_block: bool,
    bp: &mut u8,
    out: &mut Vec<u8>,
) {
    let mut pos = instart;
    loop {
        let mut blocksize: u16 = 65535;
        if pos + usize::from(blocksize) > inend {
            blocksize = (inend - pos) as u16;
        }
        let current_final = pos + usize::from(blocksize) >= inend;
        let nlen = !blocksize;

        add_bit(u32::from(final_block && current_final), bp, out);
        add_bit(0, bp, out);
        add_bit(0, bp, out);
        // Byte-align by abandoning the partial header byte. Its unused high
        // bits stay 0, and LEN/NLEN/data are appended as new bytes.
        *bp = 0;

        out.push((blocksize % 256) as u8);
        out.push(((blocksize / 256) % 256) as u8);
        out.push((nlen % 256) as u8);
        out.push(((nlen / 256) % 256) as u8);
        let end = pos + usize::from(blocksize);
        out.extend_from_slice(&in_data[pos..end]);

        if current_final {
            break;
        }
        pos = end;
    }
}

fn add_lz77_block(
    btype: i32,
    final_block: bool,
    in_data: &[u8],
    lz77: &Lz77Store,
    lstart: usize,
    lend: usize,
    bp: &mut u8,
    out: &mut Vec<u8>,
) {
    if btype == 0 {
        let length = lz77.byte_range(lstart, lend);
        let pos = if lstart == lend { 0 } else { lz77.pos[lstart] };
        let end = pos + length;
        add_non_compressed_block(in_data, pos, end, final_block, bp, out);
        return;
    }

    add_bit(u32::from(final_block), bp, out);
    add_bit((btype & 1) as u32, bp, out);
    add_bit(((btype & 2) >> 1) as u32, bp, out);

    let mut ll_lengths = [0u32; NUM_LL];
    let mut d_lengths = [0u32; NUM_D];
    if btype == 1 {
        get_fixed_tree(&mut ll_lengths, &mut d_lengths);
    } else {
        get_dynamic_lengths(lz77, lstart, lend, &mut ll_lengths, &mut d_lengths);
        add_dynamic_tree(&ll_lengths, &d_lengths, bp, out);
    }

    let mut ll_symbols = [0u32; NUM_LL];
    let mut d_symbols = [0u32; NUM_D];
    lengths_to_symbols(&ll_lengths, NUM_LL, 15, &mut ll_symbols);
    lengths_to_symbols(&d_lengths, NUM_D, 15, &mut d_symbols);

    add_lz77_data(
        lz77,
        lstart,
        lend,
        &ll_symbols,
        &ll_lengths,
        &d_symbols,
        &d_lengths,
        bp,
        out,
    );
    add_huffman_bits(ll_symbols[256], ll_lengths[256], bp, out);
}

fn add_lz77_block_auto_type(
    options: &Options,
    final_block: bool,
    in_data: &[u8],
    lz77: &Lz77Store,
    lstart: usize,
    lend: usize,
    bp: &mut u8,
    out: &mut Vec<u8>,
) {
    let uncompressedcost = calculate_block_size(lz77, lstart, lend, 0);
    let mut fixedcost = calculate_block_size(lz77, lstart, lend, 1);
    let dyncost = calculate_block_size(lz77, lstart, lend, 2);
    let expensivefixed = lz77.size() < 1000 || fixedcost <= dyncost * 1.1_f64;

    if lstart == lend {
        add_bits(u32::from(final_block), 1, bp, out);
        add_bits(1, 2, bp, out);
        add_bits(0, 7, bp, out);
        return;
    }

    let mut fixedstore = Lz77Store::new();
    if expensivefixed {
        let instart = lz77.pos[lstart];
        let inend = instart + lz77.byte_range(lstart, lend);
        let mut state = BlockState::new(*options, instart, inend, true);
        lz77_optimal_fixed(&mut state, in_data, instart, inend, &mut fixedstore);
        fixedcost = calculate_block_size(&fixedstore, 0, fixedstore.size(), 1);
    }

    if uncompressedcost < fixedcost && uncompressedcost < dyncost {
        add_lz77_block(0, final_block, in_data, lz77, lstart, lend, bp, out);
    } else if fixedcost < dyncost {
        if expensivefixed {
            add_lz77_block(
                1,
                final_block,
                in_data,
                &fixedstore,
                0,
                fixedstore.size(),
                bp,
                out,
            );
        } else {
            add_lz77_block(1, final_block, in_data, lz77, lstart, lend, bp, out);
        }
    } else {
        add_lz77_block(2, final_block, in_data, lz77, lstart, lend, bp, out);
    }
}

fn deflate_part(
    options: &Options,
    btype: i32,
    final_block: bool,
    in_data: &[u8],
    instart: usize,
    inend: usize,
    bp: &mut u8,
    out: &mut Vec<u8>,
) {
    if btype == 0 {
        add_non_compressed_block(in_data, instart, inend, final_block, bp, out);
        return;
    }
    if btype == 1 {
        let mut store = Lz77Store::new();
        let mut state = BlockState::new(*options, instart, inend, true);
        lz77_optimal_fixed(&mut state, in_data, instart, inend, &mut store);
        add_lz77_block(
            btype,
            final_block,
            in_data,
            &store,
            0,
            store.size(),
            bp,
            out,
        );
        return;
    }

    let mut splitpoints_uncompressed = Vec::new();
    let mut npoints = 0usize;
    if options.blocksplitting != 0 {
        splitpoints_uncompressed = block_split(
            options,
            in_data,
            instart,
            inend,
            options.blocksplittingmax as usize,
        );
        npoints = splitpoints_uncompressed.len();
    }

    let mut splitpoints = vec![0usize; npoints];
    let mut lz77 = Lz77Store::new();
    let mut totalcost = 0.0f64;

    for i in 0..=npoints {
        let start = if i == 0 {
            instart
        } else {
            splitpoints_uncompressed[i - 1]
        };
        let end = if i == npoints {
            inend
        } else {
            splitpoints_uncompressed[i]
        };
        let mut store = Lz77Store::new();
        let mut state = BlockState::new(*options, start, end, true);
        lz77_optimal(
            &mut state,
            in_data,
            start,
            end,
            options.numiterations,
            &mut store,
        );
        totalcost += calculate_block_size_auto_type(&store, 0, store.size());
        lz77.append_store(&store);
        if i < npoints {
            splitpoints[i] = lz77.size();
        }
    }

    if options.blocksplitting != 0 && npoints > 1 {
        let splitpoints2 = block_split_lz77(options, &lz77, options.blocksplittingmax as usize);
        let npoints2 = splitpoints2.len();
        let mut totalcost2 = 0.0f64;
        for i in 0..=npoints2 {
            let start = if i == 0 { 0 } else { splitpoints2[i - 1] };
            let end = if i == npoints2 {
                lz77.size()
            } else {
                splitpoints2[i]
            };
            totalcost2 += calculate_block_size_auto_type(&lz77, start, end);
        }
        if totalcost2 < totalcost {
            splitpoints = splitpoints2;
            npoints = npoints2;
        }
    }

    for i in 0..=npoints {
        let start = if i == 0 { 0 } else { splitpoints[i - 1] };
        let end = if i == npoints {
            lz77.size()
        } else {
            splitpoints[i]
        };
        add_lz77_block_auto_type(
            options,
            i == npoints && final_block,
            in_data,
            &lz77,
            start,
            end,
            bp,
            out,
        );
    }
}

/// Compress `in_data` and append DEFLATE blocks to `out`.
///
/// `btype` 0 stores bytes, 1 uses the fixed Huffman tree, and 2 chooses the
/// smallest of stored, fixed, and dynamic blocks. `final_block` is the C
/// `final` flag. Because [`MASTER_BLOCK_SIZE`] is 1000000, the input is split
/// into successive master chunks of that size.
pub fn deflate_into(
    options: &Options,
    btype: i32,
    final_block: bool,
    in_data: &[u8],
    bp: &mut u8,
    out: &mut Vec<u8>,
) {
    let insize = in_data.len();
    let mut i = 0usize;
    loop {
        let masterfinal = i + MASTER_BLOCK_SIZE >= insize;
        let final2 = final_block && masterfinal;
        let size = if masterfinal {
            insize - i
        } else {
            MASTER_BLOCK_SIZE
        };
        deflate_part(options, btype, final2, in_data, i, i + size, bp, out);
        i += size;
        if i >= insize {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options::default()
    }

    #[test]
    fn bits_are_packed_lsb_and_huffman_msb() {
        let mut out = Vec::new();
        let mut bp = 0u8;
        add_bits(0b100101, 6, &mut bp, &mut out);
        assert_eq!(bp, 6);
        assert_eq!(out, vec![0b0010_0101]);

        let mut out_h = Vec::new();
        let mut bp_h = 0u8;
        add_huffman_bits(0b100101, 6, &mut bp_h, &mut out_h);
        assert_eq!(bp_h, 6);
        assert_eq!(out_h, vec![0b0010_1001]);
    }

    #[test]
    fn empty_fixed_block_is_ten_bits() {
        let mut out = Vec::new();
        let mut bp = 0u8;
        add_bits(1, 1, &mut bp, &mut out);
        add_bits(1, 2, &mut bp, &mut out);
        add_bits(0, 7, &mut bp, &mut out);
        assert_eq!(out, vec![0x03, 0x00]);
        assert_eq!(bp, 2);
    }

    #[test]
    fn stored_blocks_match_c_bit_alignment() {
        let mut out = Vec::new();
        let mut bp = 0u8;
        deflate_into(&opts(), 0, true, &[], &mut bp, &mut out);
        assert_eq!(out, vec![0x01, 0x00, 0x00, 0xff, 0xff]);
        assert_eq!(bp, 0);

        out.clear();
        bp = 0;
        deflate_into(&opts(), 0, false, &[], &mut bp, &mut out);
        assert_eq!(out, vec![0x00, 0x00, 0x00, 0xff, 0xff]);

        out.clear();
        bp = 0;
        deflate_into(&opts(), 0, true, &[0xab], &mut bp, &mut out);
        assert_eq!(out, vec![0x01, 0x01, 0x00, 0xfe, 0xff, 0xab]);

        out = vec![0x15];
        bp = 5;
        deflate_into(&opts(), 0, true, &[0xab], &mut bp, &mut out);
        assert_eq!(out, vec![0x35, 0x01, 0x00, 0xfe, 0xff, 0xab]);
        assert_eq!(bp, 0);

        out = vec![0x15];
        bp = 6;
        deflate_into(&opts(), 0, true, &[0xab], &mut bp, &mut out);
        assert_eq!(out, vec![0x55, 0x00, 0x01, 0x00, 0xfe, 0xff, 0xab]);

        out = vec![0x15];
        bp = 7;
        deflate_into(&opts(), 0, true, &[0xab], &mut bp, &mut out);
        assert_eq!(out, vec![0x95, 0x00, 0x01, 0x00, 0xfe, 0xff, 0xab]);
    }

    #[test]
    fn stored_block_splits_at_65535() {
        let data: Vec<u8> = (0..65536u32).map(|i| (i.wrapping_mul(3)) as u8).collect();
        let mut out = Vec::new();
        let mut bp = 0u8;
        deflate_into(&opts(), 0, true, &data, &mut bp, &mut out);
        assert_eq!(out.len(), 65546);
        assert_eq!(&out[..8], &[0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x03, 0x06]);
        assert_eq!(&out[65540..], &[0x01, 0x01, 0x00, 0xfe, 0xff, 0xfd]);
        assert_eq!(bp, 0);
    }

    #[test]
    fn master_blocks_split_at_one_million() {
        let n = MASTER_BLOCK_SIZE;
        let data = vec![0u8; n + 1];
        let mut out = Vec::new();
        let mut bp = 0u8;
        deflate_into(&opts(), 0, true, &data, &mut bp, &mut out);
        // 1_000_000 bytes need 16 stored blocks (5 bytes overhead each) plus
        // one final stored block for the extra byte.
        assert_eq!(out.len(), n + 16 * 5 + 6);
        assert_eq!(&out[out.len() - 6..], &[0x01, 0x01, 0x00, 0xfe, 0xff, 0x00]);
        assert_eq!(out[0], 0x00);
    }

    #[test]
    fn distance_code_patch_matches_c() {
        let mut none = [0u32; 32];
        patch_distance_codes_for_buggy_decoders(&mut none);
        assert_eq!(none[0], 1);
        assert_eq!(none[1], 1);

        let mut first = [0u32; 32];
        first[0] = 5;
        patch_distance_codes_for_buggy_decoders(&mut first);
        assert_eq!(first[0], 5);
        assert_eq!(first[1], 1);

        let mut later = [0u32; 32];
        later[3] = 4;
        patch_distance_codes_for_buggy_decoders(&mut later);
        assert_eq!(later[0], 1);
        assert_eq!(later[3], 4);

        let mut two = [0u32; 32];
        two[4] = 3;
        two[9] = 2;
        patch_distance_codes_for_buggy_decoders(&mut two);
        assert_eq!(two[0], 0);
        assert_eq!(two[4], 3);
        assert_eq!(two[9], 2);
    }

    #[test]
    fn rle_population_matches_c() {
        let patterns: [[usize; 8]; 8] = [
            [1, 1, 1, 1, 1, 1, 1, 1],
            [0, 0, 0, 0, 0, 5, 5, 5],
            [10, 10, 10, 10, 1, 1, 1, 1],
            [0, 0, 0, 4, 4, 4, 4, 0],
            [7, 0, 0, 0, 0, 0, 9, 9],
            [1, 2, 3, 4, 5, 6, 7, 8],
            [100, 100, 100, 1, 1, 1, 1, 50],
            [0, 0, 0, 0, 0, 0, 0, 0],
        ];
        let expected: [[usize; 8]; 8] = [
            [1, 1, 1, 1, 1, 1, 1, 1],
            [0, 0, 0, 0, 4, 4, 4, 4],
            [10, 10, 10, 10, 1, 1, 1, 1],
            [0, 0, 0, 4, 4, 4, 4, 0],
            [7, 0, 0, 0, 0, 0, 9, 9],
            [3, 3, 3, 3, 7, 7, 7, 7],
            [100, 100, 100, 1, 1, 1, 1, 50],
            [0, 0, 0, 0, 0, 0, 0, 0],
        ];
        for (pattern, expect) in patterns.iter().zip(expected.iter()) {
            let mut counts = *pattern;
            optimize_huffman_for_rle(8, &mut counts);
            assert_eq!(&counts, expect);
        }
    }

    #[test]
    fn rle_long_histogram_matches_c() {
        let mut counts = [0usize; 288];
        for i in 0..288 {
            counts[i] = if i < 256 { (i * 17) % 23 } else { 0 };
        }
        counts[256] = 1;
        counts[10] = 0;
        counts[11] = 0;
        counts[12] = 0;
        counts[13] = 0;
        counts[14] = 0;
        optimize_huffman_for_rle(288, &mut counts);
        let expected: [usize; 288] = [
            0, 17, 11, 5, 22, 16, 10, 4, 21, 15, 0, 0, 0, 0, 0, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17,
            11, 5, 22, 16, 10, 4, 21, 15, 9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11,
            5, 22, 16, 10, 4, 21, 15, 9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 5,
            22, 16, 10, 4, 21, 15, 9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 5, 22,
            16, 10, 4, 21, 15, 9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 5, 22, 16,
            10, 4, 21, 15, 9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 5, 22, 16, 10,
            4, 21, 15, 9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 5, 22, 16, 10, 4,
            21, 15, 9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 5, 22, 16, 10, 4, 21,
            15, 9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 5, 22, 16, 10, 4, 21, 15,
            9, 3, 20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 5, 22, 16, 10, 4, 21, 15, 9, 3,
            20, 14, 8, 2, 19, 13, 7, 1, 18, 12, 6, 0, 17, 11, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(counts, expected);
    }

    #[test]
    fn fixed_and_stored_sizes_for_one_symbol() {
        let mut store = Lz77Store::new();
        store.store_lit_len_dist(97, 0, 0);
        assert_eq!(calculate_block_size(&store, 0, 1, 0), 48.0);
        assert_eq!(calculate_block_size(&store, 0, 1, 1), 18.0);

        let mut match_store = Lz77Store::new();
        match_store.store_lit_len_dist(3, 1, 0);
        assert_eq!(calculate_block_size(&match_store, 0, 1, 0), 64.0);
        assert_eq!(calculate_block_size(&match_store, 0, 1, 1), 22.0);
    }

    #[test]
    fn empty_dynamic_deflate_is_fixed_empty_block() {
        let mut out = Vec::new();
        let mut bp = 0u8;
        deflate_into(&opts(), 2, true, &[], &mut bp, &mut out);
        assert_eq!(out, vec![0x03, 0x00]);
        assert_eq!(bp, 2);
    }
}
