//! Constants from `src/zopfli/util.h`. The gcc build enables every optional
//! algorithm switch that header defines.

pub const MAX_MATCH: usize = 258;
pub const MIN_MATCH: usize = 3;
pub const NUM_LL: usize = 288;
pub const NUM_D: usize = 32;
pub const WINDOW_SIZE: usize = 32768;
pub const WINDOW_MASK: usize = WINDOW_SIZE - 1;
pub const MASTER_BLOCK_SIZE: usize = 1_000_000;
pub const LARGE_FLOAT: f64 = 1e30;
pub const CACHE_LENGTH: usize = 8;
pub const MAX_CHAIN_HITS: usize = 8192;
