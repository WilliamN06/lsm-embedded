//! Arena-allocated memtable with fixed-size B-tree
//!
//! Design decision: Use fixed-size arena instead of heap-allocated skip list.
//! Rationale: Target environments (WASM, embedded ARM) don't have a global
//! allocator in `no_std` mode. Fixed-size arena makes memory predictable.

use core::fmt::Debug;
use heapless::{FnvIndexMap, Vec};

/// Entry in the memtable — fixed size, stored in arena
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Entry<const K_SIZE: usize, const V_SIZE: usize> {
    pub key: [u8; K_SIZE],
    pub value_offset: usize,  // Offset into the value arena
    pub value_len: u16,       // Actual length (<= V_SIZE)
}

/// Memtable with arena allocation
///
/// # Type Parameters
/// - `K_SIZE`: Fixed key size in bytes (e.g., 16 for timestamp+sequence)
/// - `V_SIZE`: Maximum value size in bytes (e.g., 1KB)
/// - `CAPACITY`: Maximum number of entries (determines memory usage)
pub struct Memtable<const K_SIZE: usize, const V_SIZE: usize, const CAPACITY: usize> {
    /// Entries stored in a fixed-size vector
    entries: Vec<Entry<K_SIZE, V_SIZE>, CAPACITY>,
    /// Index mapping keys to entry indices
    index: FnvIndexMap<[u8; K_SIZE], usize, CAPACITY>,
    /// Arena for storing values
    arena: [u8; V_SIZE * CAPACITY],
    /// Current position in arena
    arena_pos: usize,
    /// Total bytes used
    total_bytes: usize,
}

impl<const K_SIZE: usize, const V_SIZE: usize, const CAPACITY: usize> 
    Memtable<K_SIZE, V_SIZE, CAPACITY> 
{
    /// Create a new empty memtable
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: FnvIndexMap::new(),
            arena: [0u8; V_SIZE * CAPACITY],
            arena_pos: 0,
            total_bytes: 0,
        }
    }

    /// Insert a key-value pair
    ///
    /// Returns `Ok(())` if inserted, `Err` if full or key exists.
    pub fn insert(&mut self, key: &[u8; K_SIZE], value: &[u8]) -> Result<(), MemtableError> {
        // Check capacity
        if self.entries.len() >= CAPACITY {
            return Err(MemtableError::Full);
        }

        // Check if key already exists
        if self.index.contains_key(key) {
            return Err(MemtableError::KeyExists);
        }

        // Check value size
        if value.len() > V_SIZE {
            return Err(MemtableError::ValueTooLarge);
        }

        // Check arena space
        if self.arena_pos + value.len() > self.arena.len() {
            return Err(MemtableError::ArenaFull);
        }

        // Store value in arena
        let offset = self.arena_pos;
        self.arena[offset..offset + value.len()].copy_from_slice(value);
        self.arena_pos += value.len();

        // Create entry
        let entry = Entry {
            key: *key,
            value_offset: offset,
            value_len: value.len() as u16,
        };

        // Store entry
        let idx = self.entries.len();
        self.entries.push(entry).map_err(|_| MemtableError::Full)?;
        self.index.insert(*key, idx).map_err(|_| MemtableError::Full)?;
        self.total_bytes += K_SIZE + value.len();

        Ok(())
    }

    /// Get a value by key
    pub fn get(&self, key: &[u8; K_SIZE]) -> Option<&[u8]> {
        let idx = *self.index.get(key)?;
        let entry = &self.entries[idx];
        let start = entry.value_offset;
        let end = start + entry.value_len as usize;
        Some(&self.arena[start..end])
    }

    /// Check if memtable is full
    pub fn is_full(&self) -> bool {
        self.entries.len() >= CAPACITY
    }

    /// Number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total bytes used (keys + values)
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Iterate over entries in insertion order
    /// Used for flushing to SSTable
    pub fn iter(&self) -> MemtableIter<K_SIZE, V_SIZE, CAPACITY> {
        MemtableIter {
            memtable: self,
            pos: 0,
        }
    }

    /// Clear the memtable (reuse arena)
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.arena_pos = 0;
        self.total_bytes = 0;
    }
}

/// Iterator over memtable entries
pub struct MemtableIter<'a, const K_SIZE: usize, const V_SIZE: usize, const CAPACITY: usize> {
    memtable: &'a Memtable<K_SIZE, V_SIZE, CAPACITY>,
    pos: usize,
}

impl<'a, const K_SIZE: usize, const V_SIZE: usize, const CAPACITY: usize> Iterator
    for MemtableIter<'a, K_SIZE, V_SIZE, CAPACITY> 
{
    type Item = (&'a [u8; K_SIZE], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.memtable.entries.len() {
            return None;
        }
        let entry = &self.memtable.entries[self.pos];
        self.pos += 1;
        let value = &self.memtable.arena[entry.value_offset..entry.value_offset + entry.value_len as usize];
        Some((&entry.key, value))
    }
}

#[derive(Debug, PartialEq)]
pub enum MemtableError {
    Full,
    KeyExists,
    ValueTooLarge,
    ArenaFull,
}

#[cfg(feature = "std")]
impl core::fmt::Display for MemtableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MemtableError::Full => write!(f, "Memtable is full"),
            MemtableError::KeyExists => write!(f, "Key already exists"),
            MemtableError::ValueTooLarge => write!(f, "Value exceeds maximum size"),
            MemtableError::ArenaFull => write!(f, "Value arena is full"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestMemtable = Memtable<16, 1024, 10>;

    #[test]
    fn test_insert_and_get() {
        let mut mt = TestMemtable::new();
        let key = [1u8; 16];
        let value = [2u8; 100];
        
        assert!(mt.insert(&key, &value).is_ok());
        assert_eq!(mt.get(&key), Some(&value[..]));
    }

    #[test]
    fn test_full_capacity() {
        let mut mt = TestMemtable::new();
        let value = [0u8; 100];
        
        for i in 0..10 {
            let mut key = [0u8; 16];
            key[0] = i;
            assert!(mt.insert(&key, &value).is_ok());
        }
        
        // 11th insert should fail
        let mut key = [0u8; 16];
        key[0] = 10;
        assert_eq!(mt.insert(&key, &value), Err(MemtableError::Full));
    }

    #[test]
    fn test_value_too_large() {
        let mut mt = TestMemtable::new();
        let key = [1u8; 16];
        let value = [2u8; 1025]; // > V_SIZE (1024)
        assert_eq!(mt.insert(&key, &value), Err(MemtableError::ValueTooLarge));
    }

    #[test]
    fn test_iteration() {
        let mut mt = TestMemtable::new();
        let value = [42u8; 50];
        
        for i in 0..5 {
            let mut key = [0u8; 16];
            key[0] = i;
            mt.insert(&key, &value).unwrap();
        }
        
        let mut count = 0;
        for (key, val) in mt.iter() {
            assert_eq!(val, &value[..]);
            assert_eq!(key[0], count);
            count += 1;
        }
        assert_eq!(count, 5);
    }

    #[test]
    fn test_clear() {
        let mut mt = TestMemtable::new();
        let key = [1u8; 16];
        let value = [2u8; 100];
        mt.insert(&key, &value).unwrap();
        assert!(!mt.is_empty());
        
        mt.clear();
        assert!(mt.is_empty());
        assert_eq!(mt.get(&key), None);
    }
}