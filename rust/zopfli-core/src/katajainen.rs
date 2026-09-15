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

//! Bounded package-merge length-limited Huffman coding (`src/zopfli/katajainen.c`).

#[derive(Clone, Copy)]
struct Node {
    weight: usize,
    tail: Option<usize>,
    count: i32,
}

fn init_node(weight: usize, count: i32, tail: Option<usize>, nodes: &mut [Node], index: usize) {
    nodes[index] = Node {
        weight,
        tail,
        count,
    };
}

fn boundary_pm(
    lists: &mut [[usize; 2]],
    leaves: &[Node],
    numsymbols: i32,
    nodes: &mut Vec<Node>,
    index: usize,
) {
    let lastcount = nodes[lists[index][1]].count;
    if index == 0 && lastcount >= numsymbols {
        return;
    }

    let newchain = nodes.len();
    nodes.push(Node {
        weight: 0,
        tail: None,
        count: 0,
    });
    let oldchain = lists[index][1];
    lists[index][0] = oldchain;
    lists[index][1] = newchain;

    if index == 0 {
        init_node(
            leaves[lastcount as usize].weight,
            lastcount + 1,
            None,
            nodes,
            newchain,
        );
    } else {
        let sum = nodes[lists[index - 1][0]].weight + nodes[lists[index - 1][1]].weight;
        if lastcount < numsymbols && sum > leaves[lastcount as usize].weight {
            let tail = nodes[oldchain].tail;
            init_node(
                leaves[lastcount as usize].weight,
                lastcount + 1,
                tail,
                nodes,
                newchain,
            );
        } else {
            let tail = Some(lists[index - 1][1]);
            init_node(sum, lastcount, tail, nodes, newchain);
            boundary_pm(lists, leaves, numsymbols, nodes, index - 1);
            boundary_pm(lists, leaves, numsymbols, nodes, index - 1);
        }
    }
}

fn boundary_pm_final(
    lists: &mut [[usize; 2]],
    leaves: &[Node],
    numsymbols: i32,
    nodes: &mut Vec<Node>,
    index: usize,
) {
    let lastcount = nodes[lists[index][1]].count;
    let sum = nodes[lists[index - 1][0]].weight + nodes[lists[index - 1][1]].weight;
    if lastcount < numsymbols && sum > leaves[lastcount as usize].weight {
        let newchain = nodes.len();
        let oldchain_tail = nodes[lists[index][1]].tail;
        nodes.push(Node {
            weight: 0,
            tail: oldchain_tail,
            count: lastcount + 1,
        });
        lists[index][1] = newchain;
    } else {
        nodes[lists[index][1]].tail = Some(lists[index - 1][1]);
    }
}

fn extract_bit_lengths(chain: usize, leaves: &[Node], nodes: &[Node], bitlengths: &mut [u32]) {
    let mut counts = [0i32; 16];
    let mut end = 16usize;
    let mut ptr = 15usize;
    let mut value = 1u32;

    let mut node = Some(chain);
    while let Some(idx) = node {
        end -= 1;
        counts[end] = nodes[idx].count;
        node = nodes[idx].tail;
    }

    let mut val = counts[15];
    while ptr >= end {
        while val > counts[ptr - 1] {
            bitlengths[leaves[(val - 1) as usize].count as usize] = value;
            val -= 1;
        }
        ptr -= 1;
        value += 1;
    }
}

/// Outputs length-limited Huffman code bitlengths.
///
/// Returns `false` on error (too few `maxbits`, or a frequency that does not
/// fit in the 9-bit count packing used by C).
pub fn length_limited_code_lengths(
    frequencies: &[usize],
    maxbits: i32,
    bitlengths: &mut [u32],
) -> bool {
    let n = frequencies.len();
    bitlengths[..n].fill(0);

    let mut leaves: Vec<Node> = Vec::with_capacity(n);
    for (i, &freq) in frequencies.iter().enumerate() {
        if freq != 0 {
            leaves.push(Node {
                weight: freq,
                tail: None,
                count: i as i32,
            });
        }
    }
    let numsymbols = leaves.len() as i32;

    if (1 << maxbits) < numsymbols {
        return false;
    }
    if numsymbols == 0 {
        return true;
    }
    if numsymbols == 1 {
        bitlengths[leaves[0].count as usize] = 1;
        return true;
    }
    if numsymbols == 2 {
        bitlengths[leaves[0].count as usize] += 1;
        bitlengths[leaves[1].count as usize] += 1;
        return true;
    }

    for leaf in &mut leaves {
        if leaf.weight >= (1usize << (usize::BITS - 9)) {
            return false;
        }
        leaf.weight = (leaf.weight << 9) | leaf.count as usize;
    }
    leaves.sort_by_key(|leaf| leaf.weight);
    for leaf in &mut leaves {
        leaf.weight >>= 9;
    }

    let mut maxbits = maxbits;
    if numsymbols - 1 < maxbits {
        maxbits = numsymbols - 1;
    }

    let mut nodes: Vec<Node> = Vec::with_capacity((maxbits as usize) * 2 * numsymbols as usize);
    let node0 = 0;
    let node1 = 1;
    nodes.push(Node {
        weight: leaves[0].weight,
        tail: None,
        count: 1,
    });
    nodes.push(Node {
        weight: leaves[1].weight,
        tail: None,
        count: 2,
    });

    let mut lists = vec![[node0, node1]; maxbits as usize];

    let num_boundary_pm_runs = 2 * numsymbols - 4;
    for _ in 0..num_boundary_pm_runs - 1 {
        boundary_pm(
            &mut lists,
            &leaves,
            numsymbols,
            &mut nodes,
            maxbits as usize - 1,
        );
    }
    boundary_pm_final(
        &mut lists,
        &leaves,
        numsymbols,
        &mut nodes,
        maxbits as usize - 1,
    );
    extract_bit_lengths(lists[maxbits as usize - 1][1], &leaves, &nodes, bitlengths);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_paper_3() {
        let input = [1, 1, 5, 7, 10, 14];
        let mut output = [0u32; 6];
        assert!(length_limited_code_lengths(&input, 3, &mut output));
        assert_eq!(output, [3, 3, 3, 3, 2, 2]);
    }

    #[test]
    fn test_from_paper_4() {
        let input = [1, 1, 5, 7, 10, 14];
        let mut output = [0u32; 6];
        assert!(length_limited_code_lengths(&input, 4, &mut output));
        assert_eq!(output, [4, 4, 3, 2, 2, 2]);
    }

    #[test]
    fn max_bits_7() {
        let input = [252, 0, 1, 6, 9, 10, 6, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut output = [0u32; 19];
        assert!(length_limited_code_lengths(&input, 7, &mut output));
        assert_eq!(
            output,
            [1, 0, 6, 4, 3, 3, 3, 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn max_bits_15() {
        let input = [
            0, 0, 0, 0, 0, 0, 18, 0, 6, 0, 12, 2, 14, 9, 27, 15, 23, 15, 17, 8, 1, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0,
        ];
        let mut output = [0u32; 32];
        assert!(length_limited_code_lengths(&input, 15, &mut output));
        assert_eq!(
            output,
            [
                0, 0, 0, 0, 0, 0, 3, 0, 5, 0, 4, 6, 4, 4, 3, 4, 3, 3, 3, 4, 6, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0
            ]
        );
    }

    #[test]
    fn only_one_frequency() {
        let input = [0, 10, 0];
        let mut output = [0u32; 3];
        assert!(length_limited_code_lengths(&input, 7, &mut output));
        assert_eq!(output, [0, 1, 0]);
    }
}
