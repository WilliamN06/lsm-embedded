use core::fmt::Debug;
use heapless::{FnvIndexMap, Vec as HeaplessVec};
use alloc::vec::Vec;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Entry<const K_SIZE: usize, const V_SIZE: usize> {
    pub key: [u8; K_SIZE],
    pub value_offset: usize,
    pub value_len: u16,
}

pub struct Memtable<const K_SIZE: usize, const V_SIZE: usize, const CAPACITY: usize> {
    entries: HeaplessVec<Entry<K_SIZE, V_SIZE>, CAPACITY>,
    index: FnvIndexMap<[u8; K_SIZE], usize, CAPACITY>,
    arena: Vec<u8>,
    arena_capacity: usize,
    arena_pos: usize,
    total_bytes: usize,
}


impl<const K_SIZE: usize, const V_SIZE: usize, const CAPACITY: usize> 
    Memtable<K_SIZE, V_SIZE, CAPACITY> 
{
    pub fn new() -> Self {
        let arena_capacity = V_SIZE * CAPACITY;
        Self {
            entries: HeaplessVec::new(),
            index: FnvIndexMap::new(),
            arena: Vec::new(),
            arena_capacity,
            arena_pos: 0,
            total_bytes: 0,
        }
    }

    pub fn insert(&mut self, key: &[u8; K_SIZE], value: &[u8]) -> Result<(), MemtableError> {
        if self.entries.len() >= CAPACITY {
            return Err(MemtableError::Full);
        }

        if self.index.contains_key(key) {
            return Err(MemtableError::KeyExists);
        }

        if value.len() > V_SIZE {
            return Err(MemtableError::ValueTooLarge);
        }

        if self.arena_pos + value.len() > self.arena_capacity {
            return Err(MemtableError::ArenaFull);
        }

        if self.arena.len() < self.arena_pos + value.len() {
            let needed = self.arena_pos + value.len();
            if needed > self.arena_capacity {
                return Err(MemtableError::ArenaFull);
            }
            self.arena.resize(needed, 0);
        }

        let offset = self.arena_pos;
        self.arena[offset..offset + value.len()].copy_from_slice(value);
        self.arena_pos += value.len();

        let entry = Entry {
            key: *key,
            value_offset: offset,
            value_len: value.len() as u16,
        };

        let idx = self.entries.len();
        self.entries.push(entry).map_err(|_| MemtableError::Full)?;
        self.index.insert(*key, idx).map_err(|_| MemtableError::Full)?;
        self.total_bytes += K_SIZE + value.len();

        Ok(())
    }

    pub fn get(&self, key: &[u8; K_SIZE]) -> Option<&[u8]> {
        let idx = *self.index.get(key)?;
        let entry = &self.entries[idx];
        let start = entry.value_offset;
        let end = start + entry.value_len as usize;
        if end <= self.arena.len() {
            Some(&self.arena[start..end])
        } else {
            None
        }
    }

    pub fn is_full(&self) -> bool {
        self.entries.len() >= CAPACITY
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn iter(&self) -> MemtableIter<K_SIZE, V_SIZE, CAPACITY> {
        MemtableIter {
            memtable: self,
            pos: 0,
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.arena.clear();
        self.arena_pos = 0;
        self.total_bytes = 0;
    }
}

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
        let start = entry.value_offset;
        let end = start + entry.value_len as usize;
        if end <= self.memtable.arena.len() {
            Some((&entry.key, &self.memtable.arena[start..end]))
        } else {
            None
        }
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

#[cfg(feature = "std")]
impl std::error::Error for MemtableError {}

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
        
        let mut key = [0u8; 16];
        key[0] = 10;
        assert_eq!(mt.insert(&key, &value), Err(MemtableError::Full));
    }

    #[test]
    fn test_value_too_large() {
        let mut mt = TestMemtable::new();
        let key = [1u8; 16];
        let value = [2u8; 1025];
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