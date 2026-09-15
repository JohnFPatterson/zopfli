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

//! Squeeze / optimal LZ77 tests that check C-compatible reconstruction.

use zopfli_core::{lz77_optimal, lz77_optimal_fixed, BlockState, Lz77Store, Options};

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
    assert_eq!(&data[instart..instart + out.len()], out.as_slice());
    out
}

#[test]
fn squeeze_matches_go_style_testdata_roundtrip() {
    // Same family of input as the Go gzip test: compressible ASCII with repeats.
    let mut input = String::from("compressthis");
    input.push_str(&"_foobar".repeat(200));
    input.push('$');
    let input = input.into_bytes();

    let mut s = BlockState::new(Options::default(), 0, input.len(), true);
    let mut store = Lz77Store::new();
    lz77_optimal(&mut s, &input, 0, input.len(), 8, &mut store);
    let out = reconstruct(&input, 0, &store);
    assert_eq!(out, input);
    assert!(
        store.size() < input.len() / 2,
        "expected squeeze to find matches, got {} symbols for {} bytes",
        store.size(),
        input.len()
    );
}

#[test]
fn squeeze_fixed_is_valid_lz77() {
    let input = b"Hello Hello Hello, Zopfli!";
    let mut s = BlockState::new(Options::default(), 0, input.len(), true);
    let mut store = Lz77Store::new();
    lz77_optimal_fixed(&mut s, input, 0, input.len(), &mut store);
    assert_eq!(reconstruct(input, 0, &store), input);
}
