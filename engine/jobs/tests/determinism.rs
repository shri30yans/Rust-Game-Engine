//! Chunked parallel helpers must match across worker counts.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use jobs::{ChunkSize, JobPool, chunk_count};
use proptest::prelude::*;

fn worker_counts() -> [usize; 4] {
    [0, 1, 2, 7]
}

fn sum_chunk(chunk_items: &[f32]) -> f32 {
    let mut acc = 0.0_f32;
    for &v in chunk_items {
        acc += v;
    }
    acc
}

proptest! {
    #[test]
    fn map_chunks_sums_match_all_worker_counts(
        len in 0usize..5000,
        chunk_raw in 1usize..300,
        values in proptest::collection::vec(-1000.0f32..1000.0, 0..5000),
    ) {
        let items: Vec<f32> = values.into_iter().take(len).collect();
        let chunk = ChunkSize::new(chunk_raw).unwrap();
        let slots = chunk_count(items.len(), chunk);
        let mut reference_bits: Option<u32> = None;

        for workers in worker_counts() {
            let pool = JobPool::new(workers).unwrap();
            let mut out = vec![0.0_f32; slots];
            pool.map_chunks(&items, chunk, &mut out, |_index, chunk_items| sum_chunk(chunk_items))
                .unwrap();

            let combined = combine_chunk_sums(&out);
            let bits = combined.to_bits();
            if let Some(expect) = reference_bits {
                prop_assert_eq!(bits, expect);
            } else {
                reference_bits = Some(bits);
            }
        }
    }

    #[test]
    fn for_each_chunk_mut_tags_match_all_worker_counts(
        len in 1usize..5000,
        chunk_raw in 1usize..300,
    ) {
        let chunk = ChunkSize::new(chunk_raw).unwrap();
        let slots = chunk_count(len, chunk);
        let mut reference_items: Option<Vec<usize>> = None;
        let mut reference_seen: Option<Vec<usize>> = None;

        for workers in worker_counts() {
            let pool = JobPool::new(workers).unwrap();
            let mut items = vec![0_usize; len];
            pool.for_each_chunk_mut(&mut items, chunk, |chunk_index, chunk_items| {
                for slot in chunk_items {
                    *slot = chunk_index;
                }
            });

            let mut seen = vec![0_usize; slots];
            pool.map_chunks(&items, chunk, &mut seen, |chunk_index, _chunk_items| chunk_index)
                .unwrap();

            for (index, &count) in seen.iter().enumerate() {
                prop_assert_eq!(count, index);
            }

            if let Some(ref expect_items) = reference_items {
                prop_assert_eq!(&items, expect_items);
                prop_assert_eq!(&seen, reference_seen.as_ref().unwrap());
            } else {
                reference_items = Some(items);
                reference_seen = Some(seen);
            }
        }
    }
}

fn combine_chunk_sums(chunk_sums: &[f32]) -> f32 {
    let mut acc = 0.0_f32;
    for &v in chunk_sums {
        acc += v;
    }
    acc
}
