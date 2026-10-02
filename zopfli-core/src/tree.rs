//! Huffman tree helpers from `src/zopfli/tree.c`.

use crate::katajainen::length_limited_code_lengths;

/// `1.0 / log(2.0)` as written in `ZopfliCalculateEntropy`, not a recomputed value.
#[allow(clippy::approx_constant)]
const K_INV_LOG2: f64 = 1.4426950408889;

/// Bit lengths for a Huffman tree, matching `ZopfliCalculateBitLengths`.
///
/// If length-limited coding returns an error, the bit lengths already written
/// are left in place.
pub fn calculate_bit_lengths(count: &[usize], n: usize, maxbits: i32, bitlengths: &mut [u32]) {
    let error = length_limited_code_lengths(count, n as i32, maxbits, bitlengths);
    debug_assert!(error == 0);
}

/// Convert code lengths to symbol codes, matching `ZopfliLengthsToSymbols`.
pub fn lengths_to_symbols(lengths: &[u32], n: usize, maxbits: u32, symbols: &mut [u32]) {
    let slots = (maxbits as usize) + 1;
    let mut bl_count = vec![0usize; slots];
    let mut next_code = vec![0usize; slots];

    for symbol in symbols.iter_mut().take(n) {
        *symbol = 0;
    }

    for bits in 0..=maxbits {
        bl_count[bits as usize] = 0;
    }
    for len in lengths.iter().take(n) {
        debug_assert!(*len <= maxbits);
        let len = *len as usize;
        bl_count[len] = bl_count[len].wrapping_add(1);
    }

    // `code` is `unsigned`. The add and shift happen after promotion to `size_t`,
    // then the result is truncated back to `unsigned`.
    let mut code: u32 = 0;
    bl_count[0] = 0;
    for bits in 1..=maxbits {
        let sum = (code as usize).wrapping_add(bl_count[(bits - 1) as usize]);
        code = sum.wrapping_shl(1) as u32;
        next_code[bits as usize] = code as usize;
    }

    for i in 0..n {
        let len = lengths[i];
        if len != 0 {
            symbols[i] = next_code[len as usize] as u32;
            next_code[len as usize] = next_code[len as usize].wrapping_add(1);
        }
    }
}

/// Entropy of each symbol, matching `ZopfliCalculateEntropy`.
///
/// `log` is the natural logarithm. The result is `log(x) * (1/log(2))` using
/// [`K_INV_LOG2`], not `f64::log2`.
pub fn calculate_entropy(count: &[usize], n: usize, bitlengths: &mut [f64]) {
    let mut sum: u32 = 0;
    for c in count.iter().take(n) {
        // `unsigned += size_t`: add in `size_t`, store the low 32 bits.
        sum = (sum as usize).wrapping_add(*c) as u32;
    }
    let log2sum = (if sum == 0 {
        (n as f64).ln()
    } else {
        f64::from(sum).ln()
    }) * K_INV_LOG2;
    for i in 0..n {
        if count[i] == 0 {
            bitlengths[i] = log2sum;
        } else {
            bitlengths[i] = log2sum - (count[i] as f64).ln() * K_INV_LOG2;
        }
        if bitlengths[i] < 0.0 && bitlengths[i] > -1e-5 {
            bitlengths[i] = 0.0;
        }
        debug_assert!(bitlengths[i] >= 0.0);
    }
}
