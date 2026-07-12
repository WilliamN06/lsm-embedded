use criterion::{criterion_group, criterion_main, Criterion};
use lsm_embedded::cache::RingCache;
use lsm_embedded::BLOCK_SIZE;

fn bench_cache_hit_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit_rate");
    
    group.bench_function("sequential_access", |b| {
        b.iter(|| {
            let mut cache = RingCache::<512>::new();
            let data = [42u8; BLOCK_SIZE];
            
            for i in 0..100 {
                cache.insert(i, &data).unwrap();
            }
            
            for i in 0..100 {
                cache.get(i).unwrap();
            }
            
            cache.hit_rate()
        });
    });
    
    group.bench_function("random_access", |b| {
        b.iter(|| {
            let mut cache = RingCache::<512>::new();
            let data = [42u8; BLOCK_SIZE];
            let mut hits = 0;
            let mut total = 0;
            
            for i in 0..200 {
                cache.insert(i, &data).unwrap();
            }
            
            for _ in 0..100 {
                let idx = (rand::random::<usize>() % 200) as u64;
                if cache.get(idx).is_some() {
                    hits += 1;
                }
                total += 1;
            }
            
            hits as f32 / total as f32
        });
    });
    
    group.finish();
}

criterion_group!(benches, bench_cache_hit_rate);
criterion_main!(benches);