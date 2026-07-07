use crate::{BLOCK_SIZE, storage::Storage};
use crate::storage::StorageError;
use core::cmp::Ordering;
use heapless::Vec;
use crc32fast::Hasher;
use core::fmt;

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

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SSTableMetadata {
    pub id: u32,
    pub block_count: u16,
    pub key_count: u32,
    pub min_key: [u8; 16],
    pub max_key: [u8; 16],
    pub total_size: u64,
}

#[derive(Clone)]
pub struct SSTable<const MAX_BLOCKS: usize> {
    pub metadata: SSTableMetadata,
    pub blocks: Vec<Block, MAX_BLOCKS>,
    pub block_offsets: Vec<u64, MAX_BLOCKS>,
}

impl<const MAX_BLOCKS: usize> fmt::Debug for SSTable<MAX_BLOCKS> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SSTable")
            .field("metadata", &self.metadata)
            .field("blocks", &self.blocks)
            .field("block_offsets", &self.block_offsets)
            .finish()
    }
}

impl<const MAX_BLOCKS: usize> SSTable<MAX_BLOCKS> {
    pub fn from_memtable<const K: usize, const V: usize, const C: usize>(
        memtable: &crate::memtable::Memtable<K, V, C>,
        id: u32,
    ) -> Self {
        let mut blocks = Vec::new();
        let mut block_offsets = Vec::new();
        let mut current_block_data = Vec::<u8, BLOCK_SIZE>::new();
        let mut key_count = 0;
        let mut min_key = [0xFFu8; 16];
        let mut max_key = [0x00u8; 16];

        for (key, value) in memtable.iter() {
            let entry_size = K + value.len() + 8;
            if current_block_data.len() + entry_size > BLOCK_SIZE && !current_block_data.is_empty() {
                let block_data = current_block_data.as_slice();
                let block = Block::new(block_data);
                blocks.push(block).unwrap_or_else(|_| panic!("Block limit exceeded"));
                current_block_data.clear();
            }

            current_block_data.extend_from_slice(&(K as u32).to_le_bytes()).unwrap();
            current_block_data.extend_from_slice(&key[..]).unwrap();
            current_block_data.extend_from_slice(&(value.len() as u32).to_le_bytes()).unwrap();
            current_block_data.extend_from_slice(value).unwrap();

            for i in 0..K.min(16) {
                if key[i] < min_key[i] { min_key[i] = key[i]; }
                if key[i] > max_key[i] { max_key[i] = key[i]; }
            }
            key_count += 1;
        }

        if !current_block_data.is_empty() {
            let block = Block::new(current_block_data.as_slice());
            blocks.push(block).unwrap_or_else(|_| panic!("Block limit exceeded"));
        }

        let mut offset = 0;
        block_offsets.resize(blocks.len(), 0).unwrap();
        for (i, _) in blocks.iter().enumerate() {
            block_offsets[i] = offset;
            offset += (BLOCK_SIZE + 4) as u64;
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

    pub fn write<STORAGE: Storage>(&self, storage: &mut STORAGE, base_offset: u64) -> Result<(), <STORAGE as Storage>::Error> {
        for (i, block) in self.blocks.iter().enumerate() {
            let offset = base_offset + self.block_offsets[i];
            block.write(storage, offset)?;
        }
        
        let metadata_offset = base_offset + self.total_size();
        let metadata_bytes = self.serialize_metadata();
        storage.write_at(metadata_offset, &metadata_bytes)?;
        
        let end_marker = [0xFF, 0xFF, 0xFF, 0xFF];
        storage.write_at(metadata_offset + metadata_bytes.len() as u64, &end_marker)
    }

    pub fn read<STORAGE: Storage>(storage: &mut STORAGE, base_offset: u64) -> Result<Self, <STORAGE as Storage>::Error> {
        let mut metadata_bytes = [0u8; 64];
        storage.read_at(base_offset, &mut metadata_bytes)?;
        
        let metadata = Self::deserialize_metadata(&metadata_bytes)
            .map_err(|e| e.into())?;
        
        let total_blocks = metadata.block_count as usize;
        let mut blocks = Vec::new();
        let mut block_offsets = Vec::new();
        
        for i in 0..total_blocks {
            let offset = base_offset + (i as u64) * (BLOCK_SIZE as u64 + 4);
            let block = Block::read(storage, offset)?;
            
            if !block.verify() {
                return Err(StorageError::Corruption.into());
            }
            
            blocks.push(block).map_err(|_| StorageError::Corruption.into())?;
            block_offsets.push(offset).map_err(|_| StorageError::Corruption.into())?;
        }
        
        Ok(Self {
            metadata,
            blocks,
            block_offsets,
        })
    }

    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let key_len = key.len().min(16);
        if key < &self.metadata.min_key[..key_len] || key > &self.metadata.max_key[..key_len] {
            return None;
        }
        
        let mut lo = 0;
        let mut hi = self.blocks.len();
        
        while lo < hi {
            let mid = (lo + hi) / 2;
            let block = &self.blocks[mid];
            
            if let Some(first_key) = Self::get_first_key_from_block(block) {
                match first_key.as_slice().cmp(key) {
                    Ordering::Equal => {
                        return Self::search_block_for_key(block, key);
                    }
                    Ordering::Less => lo = mid + 1,
                    Ordering::Greater => hi = mid,
                }
            } else {
                break;
            }
        }
        
        if lo < self.blocks.len() {
            return Self::search_block_for_key(&self.blocks[lo], key);
        }
        
        None
    }

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

    fn search_block_for_key<'a>(block: &'a Block, key: &[u8]) -> Option<&'a [u8]> {
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

    pub fn total_size(&self) -> u64 {
        self.metadata.total_size
    }

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