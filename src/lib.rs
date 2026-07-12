#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(not(feature = "std"), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod memtable;
pub mod sstable;
pub mod storage;
pub mod compaction;
pub mod cache;
pub mod bloom;
pub mod wasm;

pub use memtable::{Memtable, MemtableError};
pub use sstable::{SSTable, Block};
pub use storage::{Storage, StorageError, InMemoryStorage};
pub use compaction::{CompactionManager, CompactionStats, Level};
pub use cache::RingCache;
pub use bloom::PartitionedBloom;

pub const BLOCK_SIZE: usize = 4096;
pub const DEFAULT_KEY_SIZE: usize = 16;
pub const DEFAULT_VALUE_SIZE: usize = 128;
pub const DEFAULT_CAPACITY: usize = 8;
pub const CACHE_SLOTS: usize = 512;

#[cfg(test)]
mod tests {
    use super::*;
    use storage::InMemoryStorage;

    #[test]
    fn test_basic_write_read() {
        let mut storage = InMemoryStorage::new();
        let mut memtable = Memtable::<DEFAULT_KEY_SIZE, DEFAULT_VALUE_SIZE, DEFAULT_CAPACITY>::new();

        let key = [1u8; DEFAULT_KEY_SIZE];
        let value = b"Hello, LSM!";
        memtable.insert(&key, value).unwrap();

        assert_eq!(memtable.get(&key), Some(&value[..]));

        let sstable = SSTable::<10>::from_memtable(&memtable, 1);
        sstable.write(&mut storage, 0).unwrap();

        let read_sstable = SSTable::<10>::read(&mut storage, 0).unwrap();
        assert_eq!(read_sstable.get(&key), Some(&value[..]));
    }
}