//! Bounded package-merge from `src/zopfli/katajainen.c`.
//!
//! Returns 0 on success and 1 on the same error conditions as C. Node storage
//! is a pool of indices: `None` is a null tail, and `pool.next` is the bump
//! pointer.

/// One chain node or leaf. `tail` is an index into the pool, or `None`.
#[derive(Clone, Copy)]
struct Node {
    weight: usize,
    tail: Option<usize>,
    count: i32,
}

impl Node {
    fn empty() -> Self {
        Self {
            weight: 0,
            tail: None,
            count: 0,
        }
    }
}

/// `LeafComparator`: `(int)(a->weight - b->weight)` with `size_t` subtraction.
fn leaf_comparator(a: &Node, b: &Node) -> i32 {
    a.weight.wrapping_sub(b.weight) as i32
}

/// glibc 2.39 `qsort` for this 24-byte node is a stable mergesort
/// (`msort_with_tmp`): the left run is taken when `cmp <= 0`.
///
/// That comparator is not a strict weak order once weights differ by 2^31 or
/// more in the low 32 bits, and `slice::sort_by` panics on it. The mergesort
/// decisions are reproduced directly so the leaf order matches gcc.
fn qsort_leaves(leaves: &mut [Node]) {
    let n = leaves.len();
    if n <= 1 {
        return;
    }
    let mut tmp = vec![Node::empty(); n];
    msort_with_tmp(leaves, &mut tmp);
}

fn msort_with_tmp(b: &mut [Node], tmp: &mut [Node]) {
    let n = b.len();
    if n <= 1 {
        return;
    }
    let n1 = n / 2;
    msort_with_tmp(&mut b[..n1], tmp);
    msort_with_tmp(&mut b[n1..], tmp);

    let mut i = 0;
    let mut j = 0;
    let mut k = 0;
    let n2 = n - n1;
    while i < n1 && j < n2 {
        if leaf_comparator(&b[i], &b[n1 + j]) <= 0 {
            tmp[k] = b[i];
            i += 1;
        } else {
            tmp[k] = b[n1 + j];
            j += 1;
        }
        k += 1;
    }
    while i < n1 {
        tmp[k] = b[i];
        i += 1;
        k += 1;
    }
    b[..k].copy_from_slice(&tmp[..k]);
}

fn init_node(nodes: &mut [Node], node: usize, weight: usize, count: i32, tail: Option<usize>) {
    nodes[node].weight = weight;
    nodes[node].count = count;
    nodes[node].tail = tail;
}

fn boundary_pm(
    lists: &mut [[usize; 2]],
    leaves: &[Node],
    numsymbols: i32,
    nodes: &mut [Node],
    next: &mut usize,
    index: i32,
) {
    let lastcount = nodes[lists[index as usize][1]].count;
    if index == 0 && lastcount >= numsymbols {
        return;
    }

    let newchain = *next;
    *next += 1;
    let oldchain = lists[index as usize][1];

    // Point at the new node before the recursive calls.
    lists[index as usize][0] = oldchain;
    lists[index as usize][1] = newchain;

    if index == 0 {
        init_node(
            nodes,
            newchain,
            leaves[lastcount as usize].weight,
            lastcount + 1,
            None,
        );
    } else {
        let prev = (index - 1) as usize;
        let sum = nodes[lists[prev][0]]
            .weight
            .wrapping_add(nodes[lists[prev][1]].weight);
        if lastcount < numsymbols && sum > leaves[lastcount as usize].weight {
            let tail = nodes[oldchain].tail;
            init_node(
                nodes,
                newchain,
                leaves[lastcount as usize].weight,
                lastcount + 1,
                tail,
            );
        } else {
            init_node(nodes, newchain, sum, lastcount, Some(lists[prev][1]));
            boundary_pm(lists, leaves, numsymbols, nodes, next, index - 1);
            boundary_pm(lists, leaves, numsymbols, nodes, next, index - 1);
        }
    }
}

fn boundary_pm_final(
    lists: &mut [[usize; 2]],
    leaves: &[Node],
    numsymbols: i32,
    nodes: &mut [Node],
    next: usize,
    index: i32,
) {
    let lastcount = nodes[lists[index as usize][1]].count;
    let prev = (index - 1) as usize;
    let sum = nodes[lists[prev][0]]
        .weight
        .wrapping_add(nodes[lists[prev][1]].weight);

    if lastcount < numsymbols && sum > leaves[lastcount as usize].weight {
        let newchain = next;
        let oldchain = nodes[lists[index as usize][1]].tail;
        lists[index as usize][1] = newchain;
        // Weight is left untouched, matching the C function.
        nodes[newchain].count = lastcount + 1;
        nodes[newchain].tail = oldchain;
    } else {
        nodes[lists[index as usize][1]].tail = Some(lists[prev][1]);
    }
}

fn init_lists(
    nodes: &mut [Node],
    next: &mut usize,
    leaves: &[Node],
    maxbits: i32,
    lists: &mut [[usize; 2]],
) {
    let node0 = *next;
    *next += 1;
    let node1 = *next;
    *next += 1;
    init_node(nodes, node0, leaves[0].weight, 1, None);
    init_node(nodes, node1, leaves[1].weight, 2, None);
    for list in lists.iter_mut().take(maxbits as usize) {
        list[0] = node0;
        list[1] = node1;
    }
}

fn extract_bit_lengths(nodes: &[Node], chain: usize, leaves: &[Node], bitlengths: &mut [u32]) {
    let mut counts = [0i32; 16];
    let mut end: u32 = 16;
    let mut ptr: u32 = 15;
    let mut value: u32 = 1;

    let mut node = Some(chain);
    while let Some(idx) = node {
        end = end.wrapping_sub(1);
        counts[end as usize] = nodes[idx].count;
        node = nodes[idx].tail;
    }

    let mut val = counts[15];
    while ptr >= end {
        let prev = ptr.wrapping_sub(1) as usize;
        while val > counts[prev] {
            bitlengths[leaves[(val - 1) as usize].count as usize] = value;
            val -= 1;
        }
        ptr = ptr.wrapping_sub(1);
        value = value.wrapping_add(1);
    }
}

/// Length-limited Huffman bit lengths, matching `ZopfliLengthLimitedCodeLengths`.
///
/// `frequencies` and `bitlengths` each have at least `n` elements.
/// Returns 0 on success and 1 when `maxbits` cannot represent the symbols or a
/// weight does not leave 9 bits for the symbol index.
pub fn length_limited_code_lengths(
    frequencies: &[usize],
    n: i32,
    mut maxbits: i32,
    bitlengths: &mut [u32],
) -> i32 {
    if n <= 0 {
        return 0;
    }

    let mut numsymbols: i32 = 0;
    let mut leaves = vec![Node::empty(); n as usize];

    for i in 0..n {
        bitlengths[i as usize] = 0;
    }

    for i in 0..n {
        if frequencies[i as usize] != 0 {
            leaves[numsymbols as usize].weight = frequencies[i as usize];
            leaves[numsymbols as usize].count = i;
            numsymbols += 1;
        }
    }

    // `1 << maxbits` is a signed `int` shift. Callers pass a small non-negative
    // `maxbits` (7 or 15); shifts outside 0..31 are undefined in C.
    if (0..31).contains(&maxbits) && (1 << maxbits) < numsymbols {
        return 1;
    }
    if numsymbols == 0 {
        return 0;
    }
    if numsymbols == 1 {
        bitlengths[leaves[0].count as usize] = 1;
        return 0;
    }
    if numsymbols == 2 {
        let c0 = leaves[0].count as usize;
        let c1 = leaves[1].count as usize;
        bitlengths[c0] = bitlengths[c0].wrapping_add(1);
        bitlengths[c1] = bitlengths[c1].wrapping_add(1);
        return 0;
    }

    // Fold the symbol index into the low 9 bits, then sort lightest first.
    let weight_bits = std::mem::size_of::<usize>() * 8;
    for leaf in leaves.iter_mut().take(numsymbols as usize) {
        if leaf.weight >= (1usize << (weight_bits - 9)) {
            return 1;
        }
        leaf.weight = leaf.weight.wrapping_shl(9) | (leaf.count as usize);
    }
    qsort_leaves(&mut leaves[..numsymbols as usize]);
    for leaf in leaves.iter_mut().take(numsymbols as usize) {
        leaf.weight >>= 9;
    }

    if numsymbols - 1 < maxbits {
        maxbits = numsymbols - 1;
    }

    let pool_len = (maxbits as usize)
        .wrapping_mul(2)
        .wrapping_mul(numsymbols as usize);
    let mut nodes = vec![Node::empty(); pool_len];
    let mut next: usize = 0;
    let mut lists = vec![[0usize; 2]; maxbits as usize];
    init_lists(&mut nodes, &mut next, &leaves, maxbits, &mut lists);

    let num_boundary_pm_runs = 2 * numsymbols - 4;
    let mut i = 0;
    while i < num_boundary_pm_runs - 1 {
        boundary_pm(
            &mut lists,
            &leaves,
            numsymbols,
            &mut nodes,
            &mut next,
            maxbits - 1,
        );
        i += 1;
    }
    boundary_pm_final(
        &mut lists,
        &leaves,
        numsymbols,
        &mut nodes,
        next,
        maxbits - 1,
    );

    extract_bit_lengths(
        &nodes,
        lists[(maxbits - 1) as usize][1],
        &leaves,
        bitlengths,
    );
    0
}
