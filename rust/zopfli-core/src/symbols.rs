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

//! Utilities for the LZ77 symbols of the DEFLATE spec (`src/zopfli/symbols.h`).

const LENGTH_SYMBOL: [i32; 259] = [
    0, 0, 0, 257, 258, 259, 260, 261, 262, 263, 264, 265, 265, 266, 266, 267, 267, 268, 268, 269,
    269, 269, 269, 270, 270, 270, 270, 271, 271, 271, 271, 272, 272, 272, 272, 273, 273, 273, 273,
    273, 273, 273, 273, 274, 274, 274, 274, 274, 274, 274, 274, 275, 275, 275, 275, 275, 275, 275,
    275, 276, 276, 276, 276, 276, 276, 276, 276, 277, 277, 277, 277, 277, 277, 277, 277, 277, 277,
    277, 277, 277, 277, 277, 277, 278, 278, 278, 278, 278, 278, 278, 278, 278, 278, 278, 278, 278,
    278, 278, 278, 279, 279, 279, 279, 279, 279, 279, 279, 279, 279, 279, 279, 279, 279, 279, 279,
    280, 280, 280, 280, 280, 280, 280, 280, 280, 280, 280, 280, 280, 280, 280, 280, 281, 281, 281,
    281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 281,
    281, 281, 281, 281, 281, 281, 281, 281, 281, 281, 282, 282, 282, 282, 282, 282, 282, 282, 282,
    282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282, 282,
    282, 282, 282, 282, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283,
    283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 283, 284, 284,
    284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 284,
    284, 284, 284, 284, 284, 284, 284, 284, 284, 284, 285,
];

const LENGTH_EXTRA_BITS: [i32; 259] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 0,
];

#[allow(dead_code)]
const LENGTH_EXTRA_BITS_VALUE: [i32; 259] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0,
    1, 2, 3, 0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4,
    5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
    10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5,
    6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 0, 1, 2, 3,
    4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
    29, 30, 31, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
    23, 24, 25, 26, 27, 28, 29, 30, 0,
];

const LENGTH_SYMBOL_EXTRA_BITS: [i32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

const DIST_SYMBOL_EXTRA_BITS: [i32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Extra bits for the given distance, matching `ZopfliGetDistExtraBits`.
pub fn get_dist_extra_bits(dist: i32) -> i32 {
    if dist < 5 {
        0
    } else {
        (dist as u32 - 1).ilog2() as i32 - 1
    }
}

/// Extra-bit value for the given distance, matching `ZopfliGetDistExtraBitsValue`.
#[allow(dead_code)]
pub fn get_dist_extra_bits_value(dist: i32) -> i32 {
    if dist < 5 {
        0
    } else {
        let l = (dist as u32 - 1).ilog2();
        (dist - (1 + (1 << l))) & ((1 << (l - 1)) - 1)
    }
}

/// Distance symbol in `[0, 29]`, matching `ZopfliGetDistSymbol`.
pub fn get_dist_symbol(dist: i32) -> i32 {
    if dist < 5 {
        dist - 1
    } else {
        let l = (dist as u32 - 1).ilog2() as i32;
        let r = ((dist - 1) >> (l - 1)) & 1;
        l * 2 + r
    }
}

/// Extra bits for the given length, matching `ZopfliGetLengthExtraBits`.
pub fn get_length_extra_bits(l: i32) -> i32 {
    LENGTH_EXTRA_BITS[l as usize]
}

/// Extra-bit value for the given length, matching `ZopfliGetLengthExtraBitsValue`.
#[allow(dead_code)]
pub fn get_length_extra_bits_value(l: i32) -> i32 {
    LENGTH_EXTRA_BITS_VALUE[l as usize]
}

/// Length symbol in `[257, 285]`, matching `ZopfliGetLengthSymbol`.
pub fn get_length_symbol(l: i32) -> i32 {
    LENGTH_SYMBOL[l as usize]
}

/// Extra bits for a length symbol, matching `ZopfliGetLengthSymbolExtraBits`.
pub fn get_length_symbol_extra_bits(s: i32) -> i32 {
    LENGTH_SYMBOL_EXTRA_BITS[(s - 257) as usize]
}

/// Extra bits for a distance symbol, matching `ZopfliGetDistSymbolExtraBits`.
pub fn get_dist_symbol_extra_bits(s: i32) -> i32 {
    DIST_SYMBOL_EXTRA_BITS[s as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_symbols_cover_deflate_range() {
        assert_eq!(get_length_symbol(3), 257);
        assert_eq!(get_length_symbol(258), 285);
        assert_eq!(get_length_extra_bits(258), 0);
        assert_eq!(get_length_symbol_extra_bits(285), 0);
    }

    #[test]
    fn dist_symbols_match_first_code_boundaries() {
        assert_eq!(get_dist_symbol(1), 0);
        assert_eq!(get_dist_symbol(4), 3);
        assert_eq!(get_dist_symbol(5), 4);
        assert_eq!(get_dist_symbol(16385), 28);
        assert_eq!(get_dist_symbol(24577), 29);
        assert_eq!(get_dist_extra_bits(1), 0);
        assert_eq!(get_dist_extra_bits(32768), 13);
    }
}
