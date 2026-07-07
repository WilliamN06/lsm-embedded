use lsm_embedded::{Memtable, compaction::CompactionManager, storage::InMemoryStorage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== lsm-embedded Basic Example with Compaction ===");

    let mut storage = InMemoryStorage::new();
    let mut compactor = CompactionManager::<3, 3, 5>::new();

    for batch in 0..2 {
        println!("Batch {}: Inserting 3 entries", batch);
        let mut memtable = Memtable::<16, 128, 4>::new();
        let value = b"Test";
        
        for i in 0..3 {
            let mut key = [0u8; 16];
            key[0..4].copy_from_slice(&((batch * 10 + i) as u32).to_le_bytes());
            memtable.insert(&key, value)?;
        }
        
        compactor.flush_memtable(&memtable, &mut storage)?;
        compactor.maybe_compact(&mut storage)?;
        
        let stats = compactor.stats();
        println!("  Total: {} tables, {} bytes", 
            stats.total_tables, stats.total_size);
    }

    println!("Verifying data...");
    for batch in 0..2 {
        for i in 0..3 {
            let mut key = [0u8; 16];
            key[0..4].copy_from_slice(&((batch * 10 + i) as u32).to_le_bytes());
            if let Some(_data) = compactor.get(&key) {
                println!("  Key {}: Found", batch * 10 + i);
            }
        }
    }

    println!("Basic usage with compaction complete");
    Ok(())
}