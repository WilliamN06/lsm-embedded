use crate::{BLOCK_SIZE, storage::Storage};
use crate::storage::StorageError;
use alloc::vec::Vec;
use crc32fast::Hasher;
use core::fmt;

const METADATA_MAGIC: u32 = 0x4D534C31;
const METADATA_SIZE: usize = 64;

#[derive(Copy, Clone, Debug)]
pub struct Block {
    pub data: [u8; BLOCK_SIZE],
    pub checksum: u32,
}

impl Block {
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

    pub fn verify(&self) -> bool {
        let mut hasher = Hasher::new();
        hasher.update(&self.data);
        hasher.finalize() == self.checksum
    }

    pub fn write<STORAGE: Storage>(&self, storage: &mut STORAGE, offset: u64) -> Result<(), <STORAGE as Storage>::Error> {
        storage.write_at(offset, &self.data)?;
        let checksum_bytes = self.checksum.to_le_bytes();
        storage.write_at(offset + BLOCK_SIZE as u64, &checksum_bytes)
    }

    pub fn read<STORAGE: Storage>(storage: &mut STORAGE, offset: u64) -> Result<Self, <STORAGE as Storage>::Error> {
        let mut data = [0u8; BLOCK_SIZE];
        storage.read_at(offset, &mut data)?;
        
        let mut checksum_bytes = [0u8; 4];
        storage.read_at(offset + BLOCK_SIZE as u64, &mut checksum_bytes)?;
        let checksum = u32::from_le_bytes(checksum_bytes);
        
        Ok(Self { data, checksum })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SSTableMetadata {
    pub magic: u32,
    pub version: u32,
    pub id: u32,
    pub block_count: u32,
    pub key_count: u32,
    pub min_key: [u8; 16],
    pub max_key: [u8; 16],
    pub total_size: u64,
}

impl SSTableMetadata {
    pub fn new(id: u32, block_count: u32, key_count: u32, min_key: [u8; 16], max_key: [u8; 16], total_size: u64) -> Self {
        Self {
            magic: METADATA_MAGIC,
            version: 1,
            id,
            block_count,
            key_count,
            min_key,
            max_key,
            total_size,
        }
    }
}

#[derive(Clone)]
pub struct SSTable {
    pub metadata: SSTableMetadata,
    pub blocks: Vec<Block>,
    pub block_offsets: Vec<u64>,
}

impl fmt::Debug for SSTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SSTable")
            .field("metadata", &self.metadata)
            .field("blocks", &self.blocks.len())
            .finish()
    }
}

impl SSTable {
    pub fn from_memtable<const K: usize, const V: usize>(
        memtable: &crate::memtable::Memtable<K, V>,
        id: u32,
    ) -> Self {
        let mut blocks = Vec::new();
        let mut block_offsets = Vec::new();
        let mut current_block_data = Vec::<u8>::with_capacity(BLOCK_SIZE);
        let mut key_count = 0;
        let mut min_key = [0xFFu8; 16];
        let mut max_key = [0x00u8; 16];

        for (key, value) in memtable.iter() {
            let entry_size = K + value.len() + 8;
            if current_block_data.len() + entry_size > BLOCK_SIZE && !current_block_data.is_empty() {
                let block_data = current_block_data.as_slice();
                let block = Block::new(block_data);
                blocks.push(block);
                current_block_data.clear();
            }

            // Write: key_len (4 bytes), key (K bytes), value_len (4 bytes), value
            current_block_data.extend_from_slice(&(K as u32).to_le_bytes());
            current_block_data.extend_from_slice(&key[..]);
            current_block_data.extend_from_slice(&(value.len() as u32).to_le_bytes());
            current_block_data.extend_from_slice(value);

            for i in 0..K.min(16) {
                if key[i] < min_key[i] { min_key[i] = key[i]; }
                if key[i] > max_key[i] { max_key[i] = key[i]; }
            }
            key_count += 1;
        }

        if !current_block_data.is_empty() {
            let block = Block::new(current_block_data.as_slice());
            blocks.push(block);
        }

        let mut offset = 0;
        block_offsets.reserve(blocks.len());
        for _ in 0..blocks.len() {
            block_offsets.push(offset);
            offset += (BLOCK_SIZE + 4) as u64;
        }

        Self {
            metadata: SSTableMetadata::new(id, blocks.len() as u32, key_count, min_key, max_key, offset),
            blocks,
            block_offsets,
        }
    }

    pub fn write_at<STORAGE: Storage>(&self, storage: &mut STORAGE, base_offset: u64) -> Result<(), <STORAGE as Storage>::Error> {
        let metadata_bytes = self.serialize_metadata();
        storage.write_at(base_offset, &metadata_bytes)?;
        
        let data_start = base_offset + METADATA_SIZE as u64;
        for (i, block) in self.blocks.iter().enumerate() {
            let offset = data_start + self.block_offsets[i];
            block.write(storage, offset)?;
        }
        Ok(())
    }

    pub fn read_at<STORAGE: Storage>(storage: &mut STORAGE, base_offset: u64) -> Result<Self, <STORAGE as Storage>::Error> {
        let mut metadata_bytes = [0u8; METADATA_SIZE];
        storage.read_at(base_offset, &mut metadata_bytes)?;
        
        let metadata = Self::deserialize_metadata(&metadata_bytes)?;
        
        if metadata.magic != METADATA_MAGIC {
            return Err(StorageError::Corruption.into());
        }
        
        let total_blocks = metadata.block_count as usize;
        let mut blocks = Vec::with_capacity(total_blocks);
        let mut block_offsets = Vec::with_capacity(total_blocks);
        
        let data_start = base_offset + METADATA_SIZE as u64;
        for i in 0..total_blocks {
            let offset = data_start + (i as u64) * (BLOCK_SIZE as u64 + 4);
            let block = Block::read(storage, offset)?;
            
            if !block.verify() {
                return Err(StorageError::Corruption.into());
            }
            
            blocks.push(block);
            block_offsets.push(offset - data_start);
        }
        
        Ok(Self {
            metadata,
            blocks,
            block_offsets,
        })
    }

    pub fn write<STORAGE: Storage>(&self, storage: &mut STORAGE) -> Result<(), <STORAGE as Storage>::Error> {
        self.write_at(storage, 0)
    }

    pub fn read<STORAGE: Storage>(storage: &mut STORAGE) -> Result<Self, <STORAGE as Storage>::Error> {
        Self::read_at(storage, 0)
    }

    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        for block in &self.blocks {
            if let Some(value) = Self::find_key_in_block(block, key) {
                return Some(value);
            }
        }
        None
    }

    fn find_key_in_block<'a>(block: &'a Block, key: &[u8]) -> Option<&'a [u8]> {
        let data = &block.data;
        let mut pos = 0;
        let key_len = key.len();
        
        while pos + 4 <= data.len() {
            // Read key length from the block
            let block_key_len = u32::from_le_bytes([
                data[pos], data[pos+1], data[pos+2], data[pos+3]
            ]) as usize;
            pos += 4;
            
            if pos + block_key_len > data.len() {
                break;
            }
            
            let block_key = &data[pos..pos + block_key_len];
            pos += block_key_len;
            
            if pos + 4 > data.len() {
                break;
            }
            
            let val_len = u32::from_le_bytes([
                data[pos], data[pos+1], data[pos+2], data[pos+3]
            ]) as usize;
            pos += 4;
            
            if pos + val_len > data.len() {
                break;
            }
            
            // Compare keys
            if block_key_len == key_len && block_key == key {
                return Some(&data[pos..pos + val_len]);
            }
            
            pos += val_len;
        }
        
        None
    }

    pub fn total_size(&self) -> u64 {
        self.metadata.total_size + METADATA_SIZE as u64
    }

    fn serialize_metadata(&self) -> [u8; METADATA_SIZE] {
        let mut bytes = [0u8; METADATA_SIZE];
        let mut pos = 0;
        
        bytes[pos..pos+4].copy_from_slice(&self.metadata.magic.to_le_bytes());
        pos += 4;
        
        bytes[pos..pos+4].copy_from_slice(&self.metadata.version.to_le_bytes());
        pos += 4;
        
        bytes[pos..pos+4].copy_from_slice(&self.metadata.id.to_le_bytes());
        pos += 4;
        
        bytes[pos..pos+4].copy_from_slice(&self.metadata.block_count.to_le_bytes());
        pos += 4;
        
        bytes[pos..pos+4].copy_from_slice(&self.metadata.key_count.to_le_bytes());
        pos += 4;
        
        bytes[pos..pos+16].copy_from_slice(&self.metadata.min_key);
        pos += 16;
        
        bytes[pos..pos+16].copy_from_slice(&self.metadata.max_key);
        pos += 16;
        
        bytes[pos..pos+8].copy_from_slice(&self.metadata.total_size.to_le_bytes());
        
        bytes
    }

    fn deserialize_metadata(bytes: &[u8; METADATA_SIZE]) -> Result<SSTableMetadata, StorageError> {
        let mut pos = 0;
        
        let magic = u32::from_le_bytes([
            bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]
        ]);
        pos += 4;
        
        let version = u32::from_le_bytes([
            bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]
        ]);
        pos += 4;
        
        let id = u32::from_le_bytes([
            bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]
        ]);
        pos += 4;
        
        let block_count = u32::from_le_bytes([
            bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]
        ]);
        pos += 4;
        
        let key_count = u32::from_le_bytes([
            bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3]
        ]);
        pos += 4;
        
        let mut min_key = [0u8; 16];
        min_key.copy_from_slice(&bytes[pos..pos+16]);
        pos += 16;
        
        let mut max_key = [0u8; 16];
        max_key.copy_from_slice(&bytes[pos..pos+16]);
        pos += 16;
        
        let total_size = u64::from_le_bytes([
            bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
            bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7]
        ]);
        
        Ok(SSTableMetadata {
            magic,
            version,
            id,
            block_count,
            key_count,
            min_key,
            max_key,
            total_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memtable::Memtable;
    use crate::storage::InMemoryStorage;

    type TestMemtable = Memtable<16, 1024>;

    #[test]
    fn test_block_checksum() {
        let data = [1, 2, 3, 4, 5];
        let block = Block::new(&data);
        assert!(block.verify());
        
        let mut corrupted = block;
        corrupted.data[0] = 99;
        assert!(!corrupted.verify());
    }

    #[test]
    fn test_sstable_contains_keys() {
        let mut memtable = TestMemtable::new(10);
        let value = [42u8; 50];
        
        for i in 0..5 {
            let mut key = [0u8; 16];
            key[0] = i;
            memtable.insert(&key, &value).unwrap();
        }
        
        let sstable = SSTable::from_memtable(&memtable, 1);
        
        assert_eq!(sstable.metadata.key_count, 5);
        
        for i in 0..5 {
            let mut key = [0u8; 16];
            key[0] = i;
            let result = sstable.get(&key);
            assert!(result.is_some(), "Key {} not found in SSTable", i);
            assert_eq!(result, Some(&value[..]));
        }
    }

    #[test]
    fn test_sstable_roundtrip() {
        let mut memtable = TestMemtable::new(10);
        let value = [42u8; 50];
        
        for i in 0..5 {
            let mut key = [0u8; 16];
            key[0] = i;
            memtable.insert(&key, &value).unwrap();
        }
        
        let sstable = SSTable::from_memtable(&memtable, 1);
        let mut storage = InMemoryStorage::new();
        
        sstable.write(&mut storage).unwrap();
        let read_sstable = SSTable::read(&mut storage).unwrap();
        
        assert_eq!(read_sstable.metadata.id, 1);
        assert_eq!(read_sstable.metadata.key_count, 5);
        assert_eq!(read_sstable.metadata.block_count, sstable.metadata.block_count);
        
        for i in 0..5 {
            let mut key = [0u8; 16];
            key[0] = i;
            let result = read_sstable.get(&key);
            assert_eq!(result, Some(&value[..]), "Failed for key {}", i);
        }
    }

    #[test]
    fn test_sstable_write_at_read_at() {
        let mut memtable = TestMemtable::new(10);
        let value = [42u8; 50];
        
        for i in 0..3 {
            let mut key = [0u8; 16];
            key[0] = i;
            memtable.insert(&key, &value).unwrap();
        }
        
        let sstable = SSTable::from_memtable(&memtable, 1);
        let mut storage = InMemoryStorage::new();
        
        let offset = 1024 * 1024;
        sstable.write_at(&mut storage, offset).unwrap();
        let read_sstable = SSTable::read_at(&mut storage, offset).unwrap();
        
        assert_eq!(read_sstable.metadata.id, 1);
        assert_eq!(read_sstable.metadata.key_count, 3);
        
        for i in 0..3 {
            let mut key = [0u8; 16];
            key[0] = i;
            let result = read_sstable.get(&key);
            assert_eq!(result, Some(&value[..]), "Failed for key {}", i);
        }
    }
}