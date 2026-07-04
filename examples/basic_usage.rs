//! Basic usage example for lsm-embedded
//!
//! This demonstrates the core functionality:
//! 1. Create a memtable
//! 2. Insert some data
//! 3. Flush to SSTable
//! 4. Read it back

use lsm_embedded::{Memtable, SSTable, storage::InMemoryStorage, DEFAULT_KEY_SIZE, DEFAULT_VALUE_SIZE, DEFAULT_CAPACITY};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== lsm-embedded Basic Example ===");
    println!();

    // 1. Create storage
    let mut storage = InMemoryStorage::new();
    println!("✅ Storage created (in-memory)");

    // 2. Create memtable
    let mut memtable = Memtable::<DEFAULT_KEY_SIZE, DEFAULT_VALUE_SIZE, DEFAULT_CAPACITY>::new();
    println!("✅ Memtable created (capacity: {} entries)", DEFAULT_CAPACITY);

    // 3. Insert some data
    let value = b"Hello, LSM World!";
    for i in 0..5 {
        let mut key = [0u8; DEFAULT_KEY_SIZE];
        key[0..4].copy_from_slice(&(i as u32).to_le_bytes());
        memtable.insert(&key, value)?;
        println!("✅ Inserted key: {}", i);
    }

    // 4. Read from memtable
    let test_key = [0u8; DEFAULT_KEY_SIZE];
    if let Some(data) = memtable.get(&test_key) {
        println!("✅ Read from memtable: {:?}", core::str::from_utf8(data)?);
    }

    // 5. Flush to SSTable
    let sstable = SSTable::<10>::from_memtable(&memtable, 1);
    sstable.write(&mut storage, 0)?;
    println!("✅ Flushed to SSTable ({} blocks)", sstable.blocks.len());

    // 6. Clear memtable
    memtable.clear();
    println!("✅ Cleared memtable");

    // 7. Read from SSTable
    let read_sstable = SSTable::<10>::read(&mut storage, 0)?;
    println!("✅ Read SSTable ({} blocks, {} keys)", 
        read_sstable.blocks.len(), read_sstable.metadata.key_count);

    // 8. Verify data
    if let Some(data) = read_sstable.get(&test_key) {
        println!("✅ Verified data: {:?}", core::str::from_utf8(data)?);
    }

    // 9. Test all keys
    println!("\nReading all keys:");
    for i in 0..5 {
        let mut key = [0u8; DEFAULT_KEY_SIZE];
        key[0..4].copy_from_slice(&(i as u32).to_le_bytes());
        if let Some(data) = read_sstable.get(&key) {
            println!("  Key {}: {:?}", i, core::str::from_utf8(data)?);
        }
    }

    println!("\n✅ Basic usage test complete!");
    Ok(())
}