use crate::sstable::SSTable;
use crate::storage::Storage;
use heapless::Vec;

pub struct Level<const MAX_TABLES: usize, const MAX_BLOCKS: usize> {
    pub tables: Vec<SSTable<MAX_BLOCKS>, MAX_TABLES>,
    pub total_size: u64,
    pub level_number: usize,
}

impl<const MAX_TABLES: usize, const MAX_BLOCKS: usize> 
    Level<MAX_TABLES, MAX_BLOCKS> 
{
    pub fn new(level_number: usize) -> Self {
        Self {
            tables: Vec::new(),
            total_size: 0,
            level_number,
        }
    }

    pub fn add_table(&mut self, table: SSTable<MAX_BLOCKS>) -> Result<(), CompactionError> {
        let size = table.total_size();
        self.tables.push(table).map_err(|_| CompactionError::LevelFull)?;
        self.total_size += size;
        Ok(())
    }

    pub fn is_full(&self) -> bool {
        self.tables.len() >= MAX_TABLES
    }

    pub fn len(&self) -> usize {
        self.tables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        for table in &self.tables {
            if let Some(value) = table.get(key) {
                return Some(value);
            }
        }
        None
    }

    pub fn merge<STORAGE: Storage>(
        &mut self, 
        _storage: &mut STORAGE,
    ) -> Result<SSTable<MAX_BLOCKS>, CompactionError> {
        if self.tables.is_empty() {
            return Err(CompactionError::NoTables);
        }

        let mut merged = self.tables[0].clone();
        merged.metadata.id = self.level_number as u32 + 100;
        
        self.tables.clear();
        self.total_size = 0;
        
        Ok(merged)
    }
}

pub struct CompactionManager<const L0_CAP: usize, const L1_CAP: usize, const MAX_BLOCKS: usize> {
    levels: [Level<L0_CAP, MAX_BLOCKS>; 4],
    level_multipliers: [usize; 4],
    compacting: bool,
}

impl<const L0_CAP: usize, const L1_CAP: usize, const MAX_BLOCKS: usize> 
    CompactionManager<L0_CAP, L1_CAP, MAX_BLOCKS> 
{
    pub fn new() -> Self {
        Self {
            levels: [
                Level::new(0),
                Level::new(1),
                Level::new(2),
                Level::new(3),
            ],
            level_multipliers: [1, 10, 100, 1000],
            compacting: false,
        }
    }

    pub fn add_table<STORAGE: Storage>(
        &mut self,
        table: SSTable<MAX_BLOCKS>,
        storage: &mut STORAGE,
        base_offset: u64,
    ) -> Result<(), CompactionError> {
        table.write(storage, base_offset)
            .map_err(|_| CompactionError::WriteFailed)?;
        
        self.levels[0].add_table(table)?;
        self.maybe_compact(storage)?;
        
        Ok(())
    }

    pub fn maybe_compact<STORAGE: Storage>(
        &mut self, 
        storage: &mut STORAGE,
    ) -> Result<(), CompactionError> {
        if self.compacting {
            return Ok(());
        }
        
        for level_idx in 0..3 {
            let next_level = level_idx + 1;
            let max_size = self.get_level_max_size(level_idx);
            
            if self.levels[level_idx].total_size > max_size as u64 
                && !self.levels[level_idx].is_empty() 
            {
                self.compact_level(level_idx, next_level, storage)?;
            }
        }
        
        Ok(())
    }

    fn compact_level<STORAGE: Storage>(
        &mut self,
        from_level: usize,
        to_level: usize,
        storage: &mut STORAGE,
    ) -> Result<(), CompactionError> {
        self.compacting = true;
        
        let mut source_level = core::mem::replace(
            &mut self.levels[from_level],
            Level::new(from_level),
        );
        
        if source_level.is_empty() {
            self.compacting = false;
            return Ok(());
        }
        
        let merged = source_level.merge(storage)?;
        let offset = self.get_next_offset(storage);
        merged.write(storage, offset)
            .map_err(|_| CompactionError::WriteFailed)?;
        
        self.levels[to_level].add_table(merged)?;
        
        self.compacting = false;
        Ok(())
    }

    fn get_level_max_size(&self, level: usize) -> usize {
        let base_size = 1024 * 1024;
        base_size * self.level_multipliers[level]
    }

    fn get_next_offset<STORAGE: Storage>(&self, storage: &mut STORAGE) -> u64 {
        storage.capacity() / 4
    }

    pub fn flush_memtable<const K: usize, const V: usize, const C: usize, STORAGE: Storage>(
        &mut self,
        memtable: &crate::memtable::Memtable<K, V, C>,
        storage: &mut STORAGE,
    ) -> Result<(), CompactionError> {
        if memtable.is_empty() {
            return Ok(());
        }
        
        let id = (self.levels[0].len() + 1) as u32;
        let sstable = SSTable::<MAX_BLOCKS>::from_memtable(memtable, id);
        let offset = self.get_next_offset(storage);
        
        self.add_table(sstable, storage, offset)?;
        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        for level in &self.levels {
            if let Some(value) = level.get(key) {
                return Some(value);
            }
        }
        None
    }

    pub fn stats(&self) -> CompactionStats {
        let mut total_tables = 0;
        let mut total_size = 0;
        
        for level in &self.levels {
            total_tables += level.len();
            total_size += level.total_size;
        }
        
        CompactionStats {
            total_tables,
            total_size,
            level0_tables: self.levels[0].len(),
            level1_tables: self.levels[1].len(),
            level2_tables: self.levels[2].len(),
            level3_tables: self.levels[3].len(),
            level0_size: self.levels[0].total_size,
            level1_size: self.levels[1].total_size,
            level2_size: self.levels[2].total_size,
            level3_size: self.levels[3].total_size,
        }
    }
}

#[derive(Debug)]
pub enum CompactionError {
    LevelFull,
    WriteFailed,
    NoTables,
    MergeFailed,
}

#[cfg(feature = "std")]
impl core::fmt::Display for CompactionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CompactionError::LevelFull => write!(f, "Level is full"),
            CompactionError::WriteFailed => write!(f, "Write to storage failed"),
            CompactionError::NoTables => write!(f, "No tables to merge"),
            CompactionError::MergeFailed => write!(f, "Merge operation failed"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CompactionError {}

#[derive(Debug, Clone)]
pub struct CompactionStats {
    pub total_tables: usize,
    pub total_size: u64,
    pub level0_tables: usize,
    pub level1_tables: usize,
    pub level2_tables: usize,
    pub level3_tables: usize,
    pub level0_size: u64,
    pub level1_size: u64,
    pub level2_size: u64,
    pub level3_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Memtable, storage::InMemoryStorage, DEFAULT_KEY_SIZE, DEFAULT_VALUE_SIZE, DEFAULT_CAPACITY};

    type TestMemtable = Memtable<DEFAULT_KEY_SIZE, DEFAULT_VALUE_SIZE, DEFAULT_CAPACITY>;
    type TestCompactor = CompactionManager<5, 5, 10>;

    #[test]
    fn test_compaction_flow() {
        let mut compactor = TestCompactor::new();
        let mut storage = InMemoryStorage::new();
        let mut memtable = TestMemtable::new();
        
        let value = [42u8; 50];
        for i in 0..5 {
            let mut key = [0u8; DEFAULT_KEY_SIZE];
            key[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            memtable.insert(&key, &value).unwrap();
        }
        
        compactor.flush_memtable(&memtable, &mut storage).unwrap();
        assert_eq!(compactor.levels[0].len(), 1);
        
        let test_key = [1u8; DEFAULT_KEY_SIZE];
        assert!(compactor.get(&test_key).is_some());
        
        let stats = compactor.stats();
        assert_eq!(stats.total_tables, 1);
        assert_eq!(stats.level0_tables, 1);
    }
}