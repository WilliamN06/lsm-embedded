//! SSTable writer and reader with fixed-size blocks
//!
//! Design decision: Fixed-size 4KB blocks with embedded checksums.
//! Rationale: Matches flash page size, enables partial loading,
//! simplifies WASM memory management.

use crate::{BLOCK_SIZE, storage::Storage};
use core::cmp::Ordering;
use heapless::Vec;
use crc32fast::Hasher;

/// A single 4KB block with checksum
#[derive(Copy, Clone, Debug)]
pub struct Block {
    pub data: [u8; BLOCK_SIZE],
    pub checksum: u32,
}

impl Block {
    /// Create a new block from data (will be padded)
    pub fn new(data: &[u8]) -> Self {
        let mut block_data = [0u8; BLOCK_SIZE];
        let len = data.len().min(BLOCK_SIZE);
        block_data[..len].copy_from_slice(&data[..len]);
        
        let mut hasher = Hasher::new();
        hasher.update(&block_data);
        let checksum = hasher.finalize();
        
        Self {
            data: block_data,
            checksum,
        }
    }

    /// Verify checksum
    pub fn verify(&self) -> bool {
        let mut hasher = Hasher::new();
        hasher.update(&self.data);
        hasher.finalize() == self.checksum
    }

    /// Write this block to storage
    pub fn write<STORAGE: Storage>(&self, storage: &mut STORAGE, offset: u64) -> Result<(), STORAGE::Error> {
        storage.write_at(offset, &self.data)?;
        // Write checksum after the block (4 bytes)
        let checksum_bytes = self.checksum.to_le_bytes();
        storage.write_at(offset + BLOCK_SIZE as u64, &checksum_bytes)
    }

    /// Read a block from storage
    pub fn read<STORAGE: Storage>(storage: &mut STORAGE, offset: u64) -> Result<Self, STORAGE::Error> {
        let mut data = [0u8; BLOCK_SIZE];
        storage.read_at(offset, &mut data)?;
        
        let mut checksum_bytes = [0u8; 4];
        storage.read_at(offset + BLOCK_SIZE as u64, &mut checksum_bytes)?;
        let checksum = u32::from_le_bytes(checksum_bytes);
        
        Ok(Self { data, checksum })
    }
}

/// Metadata for an SSTable
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SSTableMetadata {
    pub id: u32,
    pub block_count: u16,
    pub key_count: u32,
    pub min_key: [u8; 16],
    pub max_key: [u8; 16],
    pub total_size: u64,
}

/// An SSTable composed of fixed-size blocks
pub struct SSTable<const MAX_BLOCKS: usize> {
    pub metadata: SSTableMetadata,
    pub blocks: Vec<Block, MAX_BLOCKS>,
    pub block_offsets: Vec<u64, MAX_BLOCKS>,
}

impl<const MAX_BLOCKS: usize> SSTable<MAX_BLOCKS> {
    /// Create a new SSTable from memtable entries
    pub fn from_memtable<K_SIZE, V_SIZE, CAPACITY>(
        memtable: &crate::memtable::Memtable<K_SIZE, V_SIZE, CAPACITY>,
        id: u32,
    ) -> Self
    where
        [(); K_SIZE]:,
    {
        let mut blocks = Vec::new();
        let mut block_offsets = Vec::new();
        let mut current_block_data = Vec::<u8, BLOCK_SIZE>::new();
        let mut key_count = 0;
        let mut min_key = [0xFFu8; 16];
        let mut max_key = [0x00u8; 16];

        // Serialize entries into blocks
        for (key, value) in memtable.iter() {
            // Check if we need a new block
            let entry_size = K_SIZE + value.len() + 8; // +8 for key/value length prefixes
            if current_block_data.len() + entry_size > BLOCK_SIZE && !current_block_data.is_empty() {
                // Flush current block
                let block_data = current_block_data.as_slice();
                let block = Block::new(block_data);
                blocks.push(block).unwrap_or_else(|_| panic!("Block limit exceeded"));
                current_block_data.clear();
            }

            // Write key length and key
            current_block_data.extend_from_slice(&(K_SIZE as u32).to_le_bytes()).unwrap();
            current_block_data.extend_from_slice(&key[..]).unwrap();
            // Write value length and value
            current_block_data.extend_from_slice(&(value.len() as u32).to_le_bytes()).unwrap();
            current_block_data.extend_from_slice(value).unwrap();

            // Update min/max keys
            for i in 0..K_SIZE.min(16) {
                if key[i] < min_key[i] { min_key[i] = key[i]; }
                if key[i] > max_key[i] { max_key[i] = key[i]; }
            }
            key_count += 1;
        }

        // Flush last block
        if !current_block_data.is_empty() {
            let block = Block::new(current_block_data.as_slice());
            blocks.push(block).unwrap_or_else(|_| panic!("Block limit exceeded"));
        }

        // Calculate offsets (sequential)
        let mut offset = 0;
        block_offsets.resize(blocks.len(), 0).unwrap();
        for (i, _) in blocks.iter().enumerate() {
            block_offsets[i] = offset;
            offset += (BLOCK_SIZE + 4) as u64;  // Block + checksum
        }

        Self {
            metadata: SSTableMetadata {
                id,
                block_count: blocks.len() as u16,
                key_count,
                min_key,
                max_key,
                total_size: offset,
            },
            blocks,
            block_offsets,
        }
    }

    /// Write the SSTable to storage
    pub fn write<STORAGE: Storage>(&self, storage: &mut STORAGE, base_offset: u64) -> Result<(), STORAGE::Error> {
        // Write each block at its offset
        for (i, block) in self.blocks.iter().enumerate() {
            let offset = base_offset + self.block_offsets[i];
            block.write(storage, offset)?;
        }
        
        // Write metadata at the end
        let metadata_offset = base_offset + self.total_size();
        let metadata_bytes = self.serialize_metadata();
        storage.write_at(metadata_offset, &metadata_bytes)?;
        
        // Write an "end of table" marker
        let end_marker = [0xFF, 0xFF, 0xFF, 0xFF];
        storage.write_at(metadata_offset + metadata_bytes.len() as u64, &end_marker)
    }

    /// Read an SSTable from storage
    pub fn read<STORAGE: Storage>(storage: &mut STORAGE, base_offset: u64) -> Result<Self, STORAGE::Error> {
        // First, read metadata to find the end
        let mut metadata_bytes = [0u8; 64];
        let metadata_start = base_offset;
        storage.read_at(metadata_start, &mut metadata_bytes)?;
        
        let metadata = Self::deserialize_metadata(&metadata_bytes)?;
        
        let total_blocks = metadata.block_count as usize;
        let mut blocks = Vec::new();
        let mut block_offsets = Vec::new();
        
        // Read each block
        for i in 0..total_blocks {
            let offset = base_offset + (i as u64) * (BLOCK_SIZE as u64 + 4);
            let block = Block::read(storage, offset)?;
            
            // Verify checksum
            if !block.verify() {
                return Err(STORAGE::Error::corruption());
            }
            
            blocks.push(block).map_err(|_| STORAGE::Error::corruption())?;
            block_offsets.push(offset).map_err(|_| STORAGE::Error::corruption())?;
        }
        
        Ok(Self {
            metadata,
            blocks,
            block_offsets,
        })
    }

    /// Get a value by key (binary search within blocks)
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        // Check if key is within range
        let key_len = key.len().min(16);
        if key < &self.metadata.min_key[..key_len] || key > &self.metadata.max_key[..key_len] {
            return None;
        }
        
        // Binary search through blocks (each block is sorted by key)
        let mut lo = 0;
        let mut hi = self.blocks.len();
        
        while lo < hi {
            let mid = (lo + hi) / 2;
            let block = &self.blocks[mid];
            
            // Read first key from block to compare
            if let Some(first_key) = Self::get_first_key_from_block(block) {
                match first_key.as_slice().cmp(key) {
                    Ordering::Equal => {
                        // Search within this block
                        return Self::search_block_for_key(block, key);
                    }
                    Ordering::Less => lo = mid + 1,
                    Ordering::Greater => hi = mid,
                }
            } else {
                break;
            }
        }
        
        // Search the final block
        if lo < self.blocks.len() {
            return Self::search_block_for_key(&self.blocks[lo], key);
        }
        
        None
    }

    /// Helper: Get first key from a block
    fn get_first_key_from_block(block: &Block) -> Option<[u8; 16]> {
        let data = &block.data;
        if data.len() < 4 {
            return None;
        }
        let key_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if key_len != 16 || data.len() < 4 + 16 {
            return None;
        }
        let mut key = [0u8; 16];
        key.copy_from_slice(&data[4..4 + 16]);
        Some(key)
    }

    /// Helper: Search a block for a key
    fn search_block_for_key(block: &Block, key: &[u8]) -> Option<&[u8]> {
        let data = &block.data;
        let mut pos = 0;
        
        while pos + 4 <= data.len() {
            let key_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            pos += 4;
            
            if pos + key_len > data.len() {
                break;
            }
            
            let block_key = &data[pos..pos + key_len];
            pos += key_len;
            
            if pos + 4 > data.len() {
                break;
            }
            
            let val_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            pos += 4;
            
            if pos + val_len > data.len() {
                break;
            }
            
            if block_key == key {
                return Some(&data[pos..pos + val_len]);
            }
            
            pos += val_len;
        }
        
        None
    }

    /// Total size of the SSTable in bytes
    pub fn total_size(&self) -> u64 {
        self.metadata.total_size
    }

    /// Clone the SSTable (for testing)
    pub fn clone(&self) -> Self {
        let mut blocks = Vec::new();
        for block in self.blocks.iter() {
            blocks.push(*block).unwrap();
        }
        let mut offsets = Vec::new();
        for offset in self.block_offsets.iter() {
            offsets.push(*offset).unwrap();
        }
        Self {
            metadata: self.metadata,
            blocks,
            block_offsets: offsets,
        }
    }

    /// Serialize metadata to bytes
    fn serialize_metadata(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[0..4].copy_from_slice(&self.metadata.id.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.metadata.block_count.to_le_bytes());
        bytes[6..10].copy_from_slice(&self.metadata.key_count.to_le_bytes());
        bytes[10..26].copy_from_slice(&self.metadata.min_key);
        bytes[26..42].copy_from_slice(&self.metadata.max_key);
        bytes[42..50].copy_from_slice(&self.metadata.total_size.to_le_bytes());
        bytes
    }

    /// Deserialize metadata from bytes
    fn deserialize_metadata(bytes: &[u8; 64]) -> Result<SSTableMetadata, StorageError> {
        Ok(SSTableMetadata {
            id: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            block_count: u16::from_le_bytes([bytes[4], bytes[5]]),
            key_count: u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]),
            min_key: {
                let mut key = [0u8; 16];
                key.copy_from_slice(&bytes[10..26]);
                key
            },
            max_key: {
                let mut key = [0u8; 16];
                key.copy_from_slice(&bytes[26..42]);
                key
            },
            total_size: u64::from_le_bytes([
                bytes[42], bytes[43], bytes[44], bytes[45],
                bytes[46], bytes[47], bytes[48], bytes[49]
            ]),
        })
    }
}

/// Storage error abstraction
#[derive(Debug, PartialEq)]
pub enum StorageError {
    Io,
    Corruption,
    Full,
    NotFound,
}

impl StorageError {
    pub fn corruption() -> Self {
        Self::Corruption
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memtable::Memtable;
    use crate::storage::InMemoryStorage;

    type TestMemtable = Memtable<16, 1024, 10>;

    #[test]
    fn test_block_checksum() {
        let data = [1, 2, 3, 4, 5];
        let block = Block::new(&data);
        assert!(block.verify());
        
        // Corrupt the data
        let mut corrupted = block;
        corrupted.data[0] = 99;
        assert!(!corrupted.verify());
    }

    #[test]
    fn test_sstable_roundtrip() {
        let mut memtable = TestMemtable::new();
        let value = [42u8; 50];
        
        for i in 0..5 {
            let mut key = [0u8; 16];
            key[0] = i;
            memtable.insert(&key, &value).unwrap();
        }
        
        let sstable = SSTable::<10>::from_memtable(&memtable, 1);
        let mut storage = InMemoryStorage::new();
        
        // Write
        sstable.write(&mut storage, 0).unwrap();
        
        // Read back
        let read_sstable = SSTable::<10>::read(&mut storage, 0).unwrap();
        
        // Verify metadata
        assert_eq!(read_sstable.metadata.id, 1);
        assert_eq!(read_sstable.metadata.key_count, 5);
        
        // Verify data
        let test_key = [1u8; 16];
        let result = read_sstable.get(&test_key);
        assert_eq!(result, Some(&value[..]));
    }

    #[test]
    fn test_sstable_binary_search() {
        let mut memtable = TestMemtable::new();
        let value = [42u8; 50];
        
        // Insert 20 entries (to ensure multiple blocks)
        for i in 0..20 {
            let mut key = [0u8; 16];
            key[0] = i;
            memtable.insert(&key, &value).unwrap();
        }
        
        let sstable = SSTable::<20>::from_memtable(&memtable, 1);
        
        // Test lookup for each key
        for i in 0..20 {
            let mut key = [0u8; 16];
            key[0] = i;
            assert_eq!(sstable.get(&key), Some(&value[..]));
        }
        
        // Test lookup for non-existent key
        let missing_key = [100u8; 16];
        assert_eq!(sstable.get(&missing_key), None);
    }
}