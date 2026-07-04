#![no_std]
#![cfg_attr(not(feature = "std"), no_main)]
#![forbid(unsafe_code)]

//! # lsm-embedded - Iteration 1: Core LSM
//!
//! This is the initial working version with:
//! - Arena-allocated memtable
//! - Fixed 4KB block SSTable
//! - In-memory storage
//! - Basic insert and get operations

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod memtable;
pub mod sstable;
pub mod storage;

// Re-export core types
pub use memtable::{Memtable, MemtableError};
pub use sstable::{SSTable, Block, BLOCK_SIZE};
pub use storage::{InMemoryStorage, Storage, StorageError};

// Constants that define our entrepreneurial tradeoffs
/// Block size: 4KB matches typical flash page size
pub const BLOCK_SIZE: usize = 4096;
/// Default key size: 16 bytes (timestamp + sequence)
pub const DEFAULT_KEY_SIZE: usize = 16;
/// Default max value size: 1024 bytes (1KB)
pub const DEFAULT_VALUE_SIZE: usize = 1024;
/// Default memtable capacity: 100 entries (approx 100KB)
pub const DEFAULT_CAPACITY: usize = 100;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_write_read() {
        let mut storage = InMemoryStorage::new();
        let mut memtable = Memtable::<DEFAULT_KEY_SIZE, DEFAULT_VALUE_SIZE, DEFAULT_CAPACITY>::new();
        
        // Insert a key-value pair
        let key = [1u8; DEFAULT_KEY_SIZE];
        let value = b"Hello, LSM!";
        memtable.insert(&key, value).unwrap();
        
        // Read it back
        assert_eq!(memtable.get(&key), Some(&value[..]));
        
        // Flush to SSTable
        let sstable = SSTable::<10>::from_memtable(&memtable, 1);
        sstable.write(&mut storage, 0).unwrap();
        
        // Read from SSTable
        let read_sstable = SSTable::<10>::read(&mut storage, 0).unwrap();
        assert_eq!(read_sstable.get(&key), Some(&value[..]));
        
        println!("✅ Basic write-read test passed!");
    }
}