use crate::BLOCK_SIZE;

struct CacheSlot {
    block_id: u64,
    data: [u8; BLOCK_SIZE],
    valid: bool,
    last_access: u32,
}

impl CacheSlot {
    fn new() -> Self {
        Self {
            block_id: 0,
            data: [0u8; BLOCK_SIZE],
            valid: false,
            last_access: 0,
        }
    }
}

pub struct RingCache<const N: usize> {
    slots: [CacheSlot; N],
    fill_count: usize,
    access_counter: u32,
    hits: usize,
    misses: usize,
}

impl<const N: usize> RingCache<N> {
    pub fn new() -> Self {
        let slots = core::array::from_fn(|_| CacheSlot::new());
        Self {
            slots,
            fill_count: 0,
            access_counter: 0,
            hits: 0,
            misses: 0,
        }
    }

    pub fn get(&mut self, block_id: u64) -> Option<&[u8]> {
        for slot in &mut self.slots {
            if slot.valid && slot.block_id == block_id {
                slot.last_access = self.access_counter;
                self.access_counter += 1;
                self.hits += 1;
                return Some(&slot.data);
            }
        }
        self.misses += 1;
        None
    }

    pub fn insert(&mut self, block_id: u64, data: &[u8]) -> Result<(), CacheError> {
        for slot in &mut self.slots {
            if slot.valid && slot.block_id == block_id {
                slot.data.copy_from_slice(data);
                slot.last_access = self.access_counter;
                self.access_counter += 1;
                return Ok(());
            }
        }

        let slot_idx = if self.fill_count < N {
            let idx = self.fill_count;
            self.fill_count += 1;
            idx
        } else {
            let mut oldest = 0;
            let mut oldest_time = u32::MAX;
            for (i, slot) in self.slots.iter().enumerate() {
                if slot.valid && slot.last_access < oldest_time {
                    oldest_time = slot.last_access;
                    oldest = i;
                }
            }
            oldest
        };

        let slot = &mut self.slots[slot_idx];
        slot.block_id = block_id;
        slot.data.copy_from_slice(data);
        slot.valid = true;
        slot.last_access = self.access_counter;
        self.access_counter += 1;

        Ok(())
    }

    pub fn len(&self) -> usize {
        self.fill_count
    }

    pub fn hit_rate(&self) -> f32 {
        if self.hits + self.misses == 0 {
            return 0.0;
        }
        self.hits as f32 / (self.hits + self.misses) as f32
    }

    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            slot.valid = false;
        }
        self.fill_count = 0;
    }
}

#[derive(Debug)]
pub enum CacheError {
    Full,
    InvalidBlock,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic() {
        let mut cache = RingCache::<4>::new();
        let block_id = 123;
        let data = [42u8; BLOCK_SIZE];

        cache.insert(block_id, &data).unwrap();
        let cached = cache.get(block_id).unwrap();
        assert_eq!(cached, &data[..]);
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = RingCache::<2>::new();

        let data1 = [1u8; BLOCK_SIZE];
        let data2 = [2u8; BLOCK_SIZE];
        let data3 = [3u8; BLOCK_SIZE];

        cache.insert(1, &data1).unwrap();
        cache.insert(2, &data2).unwrap();
        cache.insert(3, &data3).unwrap();

        let has1 = cache.get(1).is_some();
        let has2 = cache.get(2).is_some();
        let has3 = cache.get(3).is_some();

        assert!(has3);
        assert!(has1 || has2);
    }

    #[test]
    fn test_hit_rate() {
        let mut cache = RingCache::<2>::new();
        let data = [42u8; BLOCK_SIZE];

        cache.insert(1, &data).unwrap();
        cache.get(1).unwrap();
        cache.get(1).unwrap();

        assert!(cache.hit_rate() > 0.5);
    }
}