use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use lsm_embedded::{Memtable, storage::InMemoryStorage};

fn bench_memtable_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("memtable_insert");
    
    for size in [4, 8, 16].iter() {
        group.bench_with_input(BenchmarkId::new("insert", size), size, |b, &size| {
            b.iter(|| {
                let mut memtable = Memtable::<16, 128, 32>::new();
                let value = [42u8; 100];
                for i in 0..size {
                    let mut key = [0u8; 16];
                    key[0..4].copy_from_slice(&(i as u32).to_le_bytes());
                    let _ = memtable.insert(&key, &value);
                }
            });
        });
    }
    group.finish();
}

fn bench_sstable_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("sstable_write");
    
    for size in [4, 8, 16].iter() {
        group.bench_with_input(BenchmarkId::new("write", size), size, |b, &size| {
            b.iter(|| {
                let mut memtable = Memtable::<16, 128, 32>::new();
                let value = [42u8; 100];
                for i in 0..size {
                    let mut key = [0u8; 16];
                    key[0..4].copy_from_slice(&(i as u32).to_le_bytes());
                    let _ = memtable.insert(&key, &value);
                }
                
                let sstable = lsm_embedded::SSTable::<10>::from_memtable(&memtable, 1);
                let mut storage = InMemoryStorage::new();
                let _ = sstable.write(&mut storage, 0);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_memtable_insert, bench_sstable_write);
criterion_main!(benches);