use crate::sstable::SSTable;
use crate::storage::Storage;
use alloc::vec::Vec;

pub struct Level {
    pub tables: Vec<SSTable>,
    pub total_size: u64,
    pub level_number: usize,
    pub max_tables: usize,
}

impl Level {
    pub fn new(level_number: usize, max_tables: usize) -> Self {
        Self {
            tables: Vec::with_capacity(max_tables),
            total_size: 0,
            level_number,
            max_tables,
        }
    }

    pub fn add_table(&mut self, table: SSTable) -> Result<(), CompactionError> {
        let size = table.total_size();
        if self.tables.len() >= self.max_tables {
            return Err(CompactionError::LevelFull);
        }
        self.tables.push(table);
        self.total_size += size;
        Ok(())
    }

    pub fn is_full(&self) -> bool {
        self.tables.len() >= self.max_tables
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
    ) -> Result<SSTable, CompactionError> {
        if self.tables.is_empty() {
            return Err(CompactionError::NoTables);
        }

        let merged = self.tables[0].clone();
        
        self.tables.clear();
        self.total_size = 0;
        
        Ok(merged)
    }
}

pub struct CompactionManager {
    pub levels: [Level; 4],
    level_multipliers: [usize; 4],
    compacting: bool,
    level_capacity: usize,
}

impl CompactionManager {
    pub fn new(level_capacity: usize) -> Self {
        Self {
            levels: [
                Level::new(0, level_capacity),
                Level::new(1, level_capacity),
                Level::new(2, level_capacity),
                Level::new(3, level_capacity),
            ],
            level_multipliers: [1, 10, 100, 1000],
            compacting: false,
            level_capacity,
        }
    }

    pub fn flush_memtable<const K: usize, const V: usize, STORAGE: Storage>(
        &mut self,
        memtable: &crate::memtable::Memtable<K, V>,
        storage: &mut STORAGE,
    ) -> Result<(), CompactionError> {
        if memtable.is_empty() {
            return Ok(());
        }
        
        let id = (self.levels[0].len() + 1) as u32;
        let sstable = SSTable::from_memtable(memtable, id);
        
        let offset = (id as u64) * 1024 * 1024;
        sstable.write_at(storage, offset)
            .map_err(|_| CompactionError::WriteFailed)?;
        
        self.levels[0].add_table(sstable)?;
        
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
        
        let level_capacity = self.level_capacity;
        let mut source_level = core::mem::replace(
            &mut self.levels[from_level],
            Level::new(from_level, level_capacity),
        );
        
        if source_level.is_empty() {
            self.compacting = false;
            return Ok(());
        }
        
        let merged = source_level.merge(storage)?;
        
        let id = (self.levels[to_level].len() + 1) as u32 + 100;
        let offset = (id as u64) * 1024 * 1024;
        merged.write_at(storage, offset)
            .map_err(|_| CompactionError::WriteFailed)?;
        
        self.levels[to_level].add_table(merged)?;
        
        self.compacting = false;
        Ok(())
    }

    fn get_level_max_size(&self, level: usize) -> usize {
        let base_size = 1024 * 1024;
        base_size * self.level_multipliers[level]
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
    use crate::{Memtable, storage::InMemoryStorage};

    type TestMemtable = Memtable<16, 1024>;

    // Helper to create consistent keys
    fn make_key(value: u32) -> [u8; 16] {
        let mut key = [0u8; 16];
        key[0..4].copy_from_slice(&value.to_le_bytes());
        key
    }

    #[test]
    fn test_compaction_flow() {
        let mut compactor = CompactionManager::new(5);
        let mut storage = InMemoryStorage::new();
        let mut memtable = TestMemtable::new(10);
        
        let value = [42u8; 50];
        
        // Insert 5 entries with keys 0..4
        for i in 0..5 {
            let key = make_key(i);
            memtable.insert(&key, &value).unwrap();
        }
        
        // Use the same key format for searching
        let test_key = make_key(1);
        
        // Verify memtable has data
        let memtable_result = memtable.get(&test_key);
        assert!(memtable_result.is_some(), "Memtable should have the key");
        assert_eq!(memtable_result, Some(&value[..]));
        
        // Flush memtable to level 0
        compactor.flush_memtable(&memtable, &mut storage).unwrap();
        
        // Verify table is in level 0
        assert_eq!(compactor.levels[0].len(), 1);
        
        // Verify we can read the data from the compaction manager
        let result = compactor.get(&test_key);
        assert!(result.is_some(), "Key not found in compaction manager");
        assert_eq!(result, Some(&value[..]));
        
        let stats = compactor.stats();
        assert_eq!(stats.total_tables, 1);
        assert_eq!(stats.level0_tables, 1);
    }
}