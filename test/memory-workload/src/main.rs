use std::alloc::System;
use std::collections::HashMap;
use std::env;
use std::hint::black_box;
use std::mem::size_of;
use std::time::Instant;

use trace_alloc::TraceAllocator;

#[global_allocator]
static GLOBAL: TraceAllocator<System> = TraceAllocator::new(System);

const DEFAULT_MIB: usize = 128;
const TREE_DEPTH: usize = 10;
const TREE_BRANCHING: usize = 2;
const MATRIX_LEAF: usize = 8192;
const WORD_DEPTH: usize = 7;

struct Workload {
    matrix: Vec<f64>,
    blob: Vec<u8>,
    tree: TreeNode,
    corpus: Vec<String>,
    joined: String,
    index: HashMap<u64, usize>,
}

struct TreeNode {
    tag: u64,
    payload: Vec<u64>,
    children: Vec<TreeNode>,
}

struct TreeStats {
    nodes: usize,
    payload_bytes: usize,
    checksum: u64,
}

struct CorpusStats {
    total_bytes: usize,
    joined_bytes: usize,
    index_entries: usize,
}

#[inline(always)]
fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[inline(never)]
fn mix(left: u64, right: u64, depth: usize) -> u64 {
    splitmix64(left ^ right.rotate_left((depth as u32) & 63))
}

fn main() {
    trace_alloc::elf::record_loaded_elfs();
    let mib = env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            env::var("TRACE_ALLOC_TEST_MIB")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(DEFAULT_MIB)
        .max(8);

    let started = Instant::now();
    let workload = run_workload(mib);
    let build_time = started.elapsed();

    let tree_stats = inspect_tree(&workload.tree);
    let corpus_stats = inspect_corpus(&workload);

    black_box(&workload);

    println!("memory budget: {mib} MiB");
    println!("workload build time: {build_time:?}");
    println!("matrix elements: {}", workload.matrix.len());
    println!("matrix bytes: {}", workload.matrix.len() * size_of::<f64>());
    println!("blob bytes: {}", workload.blob.len());
    println!("tree nodes: {}", tree_stats.nodes);
    println!("tree payload bytes: {}", tree_stats.payload_bytes);
    println!("tree checksum: 0x{:016x}", tree_stats.checksum);
    println!("corpus strings: {}", workload.corpus.len());
    println!("corpus bytes: {}", corpus_stats.total_bytes);
    println!("joined bytes: {}", corpus_stats.joined_bytes);
    println!("index entries: {}", corpus_stats.index_entries);

    if let Ok(path) = env::var("TRACE_ALLOC_DUMP_PATH") {
        println!("trace sample dump: {path}");
    } else {
        println!("trace sample dump: disabled, set trace-alloc environment variables");
    }
}

#[inline(never)]
fn run_workload(mib: usize) -> Workload {
    let budget = mib.saturating_mul(1024).saturating_mul(1024);
    let matrix_bytes = budget / 3;
    let blob_bytes = budget / 3;
    let tree_bytes = budget / 6;
    let corpus_bytes = budget.saturating_sub(matrix_bytes + blob_bytes + tree_bytes);

    let matrix = build_matrix(matrix_bytes);
    let blob = build_blob(blob_bytes);
    let tree = build_tree(tree_bytes);
    let corpus = build_corpus(corpus_bytes);
    let joined = join_corpus(&corpus);
    let index = index_corpus(&corpus);

    Workload {
        matrix,
        blob,
        tree,
        corpus,
        joined,
        index,
    }
}

#[inline(never)]
fn build_matrix(bytes: usize) -> Vec<f64> {
    let mut elements = (bytes / size_of::<f64>()).max(MATRIX_LEAF);
    let stride = 1 << 16;
    elements = (elements / stride).max(1) * stride;

    let mut matrix = vec![0.0f64; elements];
    seed_matrix(&mut matrix);
    let checksum = transform_matrix(&mut matrix);
    black_box(checksum);
    matrix
}

#[inline(never)]
fn seed_matrix(data: &mut [f64]) {
    let mut state = 0x243f_6a88_85a3_08d3;
    for value in data.iter_mut() {
        state = splitmix64(state);
        *value = (state >> 11) as f64 / ((1u64 << 53) as f64);
    }
}

#[inline(never)]
fn transform_matrix(data: &mut [f64]) -> u64 {
    transform_partition(data, 0)
}

#[inline(never)]
fn transform_partition(data: &mut [f64], depth: usize) -> u64 {
    if data.len() <= MATRIX_LEAF {
        return transform_leaf(data, depth);
    }

    let mid = data.len() / 2;
    let (left, right) = data.split_at_mut(mid);
    let left_hash = transform_partition(left, depth + 1);
    let right_hash = transform_partition(right, depth + 1);
    mix(left_hash, right_hash, depth)
}

#[inline(never)]
fn transform_leaf(data: &mut [f64], depth: usize) -> u64 {
    let mut scratch = Vec::with_capacity(data.len());
    let mut state = (data.len() as u64) ^ ((depth as u64) << 32) ^ 0x1319_8a2e_0370_7344;

    for (index, value) in data.iter().enumerate() {
        state = splitmix64(state ^ index as u64);
        let weight = (state >> 32) as f64 / (u32::MAX as f64);
        scratch.push(value.mul_add(weight, value * 0.5));
    }

    for (slot, value) in data.iter_mut().zip(scratch.iter()) {
        *slot = *value * 0.75 + slot.fract() * 0.25;
    }

    hash_f64s(&scratch, state)
}

#[inline(never)]
fn hash_f64s(data: &[f64], seed: u64) -> u64 {
    let mut hash = seed ^ data.len() as u64;
    for value in data {
        hash = splitmix64(hash ^ value.to_bits());
    }
    hash
}

#[inline(never)]
fn build_blob(bytes: usize) -> Vec<u8> {
    let len = bytes.max(1024 * 1024);
    let mut blob = vec![0u8; len];
    fill_blob(&mut blob);
    let checksum = hash_blob(&blob);
    black_box(checksum);
    blob
}

#[inline(never)]
fn fill_blob(blob: &mut [u8]) {
    let mut state = 0xa409_3822_299f_31d0;
    for chunk in blob.chunks_mut(size_of::<u64>()) {
        state = splitmix64(state);
        let bytes = state.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

#[inline(never)]
fn hash_blob(blob: &[u8]) -> u64 {
    hash_partition(blob, 0)
}

#[inline(never)]
fn hash_partition(data: &[u8], depth: usize) -> u64 {
    if data.len() <= 64 * 1024 {
        return hash_chunk(data, depth);
    }

    let mid = data.len() / 2;
    let (left, right) = data.split_at(mid);
    let left_hash = hash_partition(left, depth + 1);
    let right_hash = hash_partition(right, depth + 1);
    mix(left_hash, right_hash, depth)
}

#[inline(never)]
fn hash_chunk(data: &[u8], depth: usize) -> u64 {
    let mut hash = 0xc0ac_29b7_c97c_50dd ^ ((depth as u64) << 48) ^ data.len() as u64;
    for chunk in data.chunks(8) {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        hash = splitmix64(hash ^ u64::from_le_bytes(word));
    }
    hash
}

#[inline(never)]
fn build_tree(bytes: usize) -> TreeNode {
    let node_count = (1usize << (TREE_DEPTH + 1)) - 1;
    let payload_words = (bytes / node_count / size_of::<u64>()).max(8);
    build_node(TREE_DEPTH, payload_words, 0x4528_21e6_38d0_1377)
}

#[inline(never)]
fn build_node(depth: usize, payload_words: usize, seed: u64) -> TreeNode {
    let mut children = Vec::with_capacity(if depth == 0 { 0 } else { TREE_BRANCHING });

    if depth > 0 {
        for branch in 0..TREE_BRANCHING {
            let child_seed = splitmix64(seed ^ (branch as u64).wrapping_mul(0x9e37_79b9));
            children.push(build_node(depth - 1, payload_words, child_seed));
        }
    }

    let mut payload = Vec::with_capacity(payload_words);
    let mut state = seed;
    for _ in 0..payload_words {
        state = splitmix64(state ^ payload.len() as u64);
        payload.push(state);
    }

    TreeNode {
        tag: seed,
        payload,
        children,
    }
}

#[inline(never)]
fn inspect_tree(root: &TreeNode) -> TreeStats {
    let mut stats = TreeStats {
        nodes: 0,
        payload_bytes: 0,
        checksum: 0x1319_8a2e_0370_7344,
    };
    walk_tree(root, 0, &mut stats);
    stats
}

#[inline(never)]
fn walk_tree(node: &TreeNode, depth: usize, stats: &mut TreeStats) {
    stats.nodes += 1;
    stats.payload_bytes += node.payload.len() * size_of::<u64>();
    stats.checksum = splitmix64(
        stats.checksum ^ node.tag.rotate_left((depth as u32) & 63) ^ node.payload.len() as u64,
    );

    for child in &node.children {
        walk_tree(child, depth + 1, stats);
    }
}

#[inline(never)]
fn build_corpus(bytes: usize) -> Vec<String> {
    let target = bytes.max(1024 * 1024);
    let count = (target / 4096).clamp(256, 2048);
    let word_len = (target / count).max(64);
    let mut corpus = Vec::with_capacity(count);

    for index in 0..count {
        let seed = splitmix64(0x082e_fa98_ec4e_6c89 ^ index as u64);
        corpus.push(build_word(WORD_DEPTH, word_len, seed));
    }

    corpus.sort();
    corpus
}

#[inline(never)]
fn build_word(remaining_depth: usize, len: usize, seed: u64) -> String {
    if remaining_depth == 0 {
        let mut word = String::with_capacity(len);
        fill_word(&mut word, seed, len);
        return word;
    }

    let word = build_word(remaining_depth - 1, len, splitmix64(seed));
    black_box(word.len());
    word
}

#[inline(never)]
fn fill_word(word: &mut String, mut state: u64, len: usize) {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    for _ in 0..len {
        state = splitmix64(state);
        let index = (state as usize) % ALPHABET.len();
        word.push(ALPHABET[index] as char);
    }
}

#[inline(never)]
fn join_corpus(corpus: &[String]) -> String {
    corpus.join(" | ")
}

#[inline(never)]
fn index_corpus(corpus: &[String]) -> HashMap<u64, usize> {
    let mut index = HashMap::with_capacity(corpus.len());
    for (position, word) in corpus.iter().enumerate() {
        index.insert(hash_word(word), position);
    }
    index
}

#[inline(never)]
fn hash_word(word: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for byte in word.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[inline(never)]
fn inspect_corpus(workload: &Workload) -> CorpusStats {
    let total_bytes = workload.corpus.iter().map(|word| word.len()).sum::<usize>();
    let index_checksum = workload.index.iter().fold(0u64, |acc, (hash, position)| {
        splitmix64(acc ^ *hash ^ *position as u64)
    });
    black_box(index_checksum);

    CorpusStats {
        total_bytes,
        joined_bytes: workload.joined.len(),
        index_entries: workload.index.len(),
    }
}
