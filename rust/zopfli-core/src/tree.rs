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

//! Huffman tree helpers (`src/zopfli/tree.c`).

use crate::katajainen::length_limited_code_lengths;

/// Bitlengths from symbol frequencies, matching `ZopfliCalculateBitLengths`.
pub fn calculate_bit_lengths(count: &[usize], maxbits: i32, bitlengths: &mut [u32]) {
    let ok = length_limited_code_lengths(count, maxbits, bitlengths);
    debug_assert!(ok);
}

/// Entropy of each symbol in bits, matching `ZopfliCalculateEntropy`.
pub fn calculate_entropy(count: &[usize], bitlengths: &mut [f64]) {
    const K_INV_LOG2: f64 = 1.4426950408889; // 1.0 / log(2.0) as in C
    let n = count.len();
    let mut sum: u32 = 0;
    for &c in count {
        sum = sum.wrapping_add(c as u32);
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
