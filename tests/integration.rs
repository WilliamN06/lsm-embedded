use lsm_embedded::{
    Memtable, SSTable, 
    compaction::CompactionManager,
    storage::InMemoryStorage,
};

type TestMemtable = Memtable<16, 1024>;

fn make_key(value: u32) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[0..4].copy_from_slice(&value.to_le_bytes());
    key
}

#[test]
fn test_full_write_read_cycle() {
    let mut storage = InMemoryStorage::new();
    let mut memtable = TestMemtable::new(10);
    let value = b"test_value";
    
    for i in 0..8 {
        let key = make_key(i);
        memtable.insert(&key, value).unwrap();
    }
    
    let sstable = SSTable::from_memtable(&memtable, 1);
    sstable.write(&mut storage).unwrap();
    
    let read_sstable = SSTable::read(&mut storage).unwrap();
    for i in 0..8 {
        let key = make_key(i);
        assert_eq!(read_sstable.get(&key), Some(&value[..]));
    }
}

#[test]
fn test_cache_performance() {
    use lsm_embedded::cache::RingCache;
    use lsm_embedded::BLOCK_SIZE;
    
    let mut cache = RingCache::<4>::new();
    let data = [42u8; BLOCK_SIZE];
    
    cache.insert(1, &data).unwrap();
    assert!(cache.get(1).is_some());
    
    cache.get(1).unwrap();
    cache.get(1).unwrap();
    assert!(cache.hit_rate() > 0.5);
}

#[test]
fn test_bloom_filter_accuracy() {
    use lsm_embedded::bloom::PartitionedBloom;
    
    let mut filter = PartitionedBloom::new(10, 3);
    
    for i in 0..5 {
        let key = format!("key{}", i);
        filter.insert(i, key.as_bytes()).unwrap();
    }
    
    for i in 0..5 {
        let key = format!("key{}", i);
        assert!(filter.might_contain(i, key.as_bytes()).unwrap());
    }
    
    let mut false_positives = 0;
    for i in 5..10 {
        let key = format!("key{}", i);
        if let Ok(contains) = filter.might_contain(i, key.as_bytes()) {
            if contains {
                false_positives += 1;
            }
        }
    }
    
    assert!(false_positives <= 2);
}

#[test]
fn test_compaction_integration() {
    let mut storage = InMemoryStorage::new();
    let mut compactor = CompactionManager::new(5);
    
    let value = b"test_data";
    let mut expected_keys = Vec::new();
    
    // Batch 0: Insert keys 0..3
    {
        let mut memtable = TestMemtable::new(10);
        for i in 0..4 {
            let key = make_key(i);
            expected_keys.push(i);
            memtable.insert(&key, value).unwrap();
        }
        
        compactor.flush_memtable(&memtable, &mut storage).unwrap();
    }
    
    // Batch 1: Insert keys 10..13
    {
        let mut memtable = TestMemtable::new(10);
        for i in 10..14 {
            let key = make_key(i);
            expected_keys.push(i);
            memtable.insert(&key, value).unwrap();
        }
        
        compactor.flush_memtable(&memtable, &mut storage).unwrap();
        compactor.maybe_compact(&mut storage).unwrap();
    }
    
    // Verify only the keys we actually inserted
    for &i in &expected_keys {
        let key = make_key(i);
        assert_eq!(compactor.get(&key), Some(&value[..]), "Key {} should be found", i);
    }
    
    // Verify that keys we didn't insert are not found (4..9 and 14..)
    for i in 4..10 {
        let key = make_key(i);
        assert!(compactor.get(&key).is_none(), "Key {} should NOT be found", i);
    }
    
    let stats = compactor.stats();
    assert_eq!(stats.total_tables, 2);
}