use lsm_embedded::{Memtable, SSTable, storage::InMemoryStorage};

type TestMemtable = Memtable<16, 1024>;

fn make_key(value: u32) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[0..4].copy_from_slice(&value.to_le_bytes());
    key
}

#[test]
fn test_in_memory_persistence() {
    let mut storage = InMemoryStorage::new();
    let mut memtable = TestMemtable::new(10);
    let value = b"persistent_data";
    
    let key = make_key(1);
    memtable.insert(&key, value).unwrap();
    
    let sstable = SSTable::from_memtable(&memtable, 1);
    sstable.write(&mut storage).unwrap();
    
    let read_sstable = SSTable::read(&mut storage).unwrap();
    assert_eq!(read_sstable.get(&key), Some(&value[..]));
}

#[test]
fn test_multiple_sstables() {
    let mut storage = InMemoryStorage::new();
    let value = b"test_data";
    
    // Write 3 SSTables at different offsets using write_at
    for id in 0..3 {
        let mut memtable = TestMemtable::new(10);
        let key = make_key(id);
        memtable.insert(&key, value).unwrap();
        
        let sstable = SSTable::from_memtable(&memtable, id);
        let offset = (id as u64) * 1024 * 1024; // 1MB apart
        sstable.write_at(&mut storage, offset).unwrap();
    }
    
    // Read each SSTable back from its offset
    for id in 0..3 {
        let offset = (id as u64) * 1024 * 1024;
        let read_sstable = SSTable::read_at(&mut storage, offset).unwrap();
        let key = make_key(id);
        assert_eq!(read_sstable.get(&key), Some(&value[..]));
    }
}