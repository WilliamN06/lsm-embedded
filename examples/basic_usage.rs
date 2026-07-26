use lsm_embedded::{Memtable, compaction::CompactionManager, storage::InMemoryStorage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== lsm-embedded Basic Example with Compaction ===");

    let mut storage = InMemoryStorage::new();
    let mut compactor = CompactionManager::new(5);

    for batch in 0..3 {
        println!("Batch {}: Inserting 10 entries", batch);
        let mut memtable = Memtable::<16, 1024>::new(10);
        let value = b"Test Data";
        
        for i in 0..10 {
            let mut key = [0u8; 16];
            key[0..4].copy_from_slice(&((batch * 10 + i) as u32).to_le_bytes());
            memtable.insert(&key, value)?;
        }
        
        compactor.flush_memtable(&memtable, &mut storage)?;
        compactor.maybe_compact(&mut storage)?;
        
        let stats = compactor.stats();
        println!("  Level 0: {} tables ({} bytes)", 
            stats.level0_tables, stats.level0_size);
        println!("  Level 1: {} tables ({} bytes)", 
            stats.level1_tables, stats.level1_size);
        println!("  Total: {} tables, {} bytes\n", 
            stats.total_tables, stats.total_size);
    }

    println!("Verifying data...");
    for batch in 0..3 {
        for i in 0..10 {
            let mut key = [0u8; 16];
            key[0..4].copy_from_slice(&((batch * 10 + i) as u32).to_le_bytes());
            if let Some(_data) = compactor.get(&key) {
                println!("  Key {}: Found", batch * 10 + i);
            } else {
                println!("  Key {}: MISSING!", batch * 10 + i);
            }
        }
    }

    println!("Basic usage with compaction complete");
    Ok(())
}