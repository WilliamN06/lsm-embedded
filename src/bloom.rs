use alloc::vec::Vec;

pub struct BloomPartition {
    bits: u64,
    hash_count: u8,
}

impl BloomPartition {
    pub fn new(hash_count: u8) -> Self {
        Self {
            bits: 0,
            hash_count: hash_count.min(6),
        }
    }

    pub fn insert(&mut self, key: &[u8]) {
        for i in 0..self.hash_count {
            let hash = self.hash_key(key, i);
            self.bits |= 1 << (hash % 64);
        }
    }

    pub fn might_contain(&self, key: &[u8]) -> bool {
        for i in 0..self.hash_count {
            let hash = self.hash_key(key, i);
            if (self.bits & (1 << (hash % 64))) == 0 {
                return false;
            }
        }
        true
    }

    fn hash_key(&self, key: &[u8], seed: u8) -> usize {
        let mut hash = 0u64;
        for byte in key {
            hash = hash.wrapping_mul(31)
                .wrapping_add((*byte as u64) ^ (seed as u64));
            hash = hash.rotate_left(7);
        }
        hash = hash.wrapping_mul(0x9e3779b97f4a7c15);
        hash as usize
    }

    pub fn memory_usage(&self) -> usize {
        8
    }
}

pub struct PartitionedBloom {
    partitions: Vec<BloomPartition>,
    loaded: Vec<bool>,
    hash_count: u8,
    max_partitions: usize,
}

impl PartitionedBloom {
    pub fn new(max_partitions: usize, hash_count: u8) -> Self {
        Self {
            partitions: Vec::with_capacity(max_partitions),
            loaded: vec![false; max_partitions],
            hash_count: hash_count.min(6),
            max_partitions,
        }
    }

    pub fn load_partition(&mut self, block_index: usize) -> Result<(), BloomError> {
        if block_index >= self.max_partitions {
            return Err(BloomError::IndexOutOfBounds);
        }

        if self.loaded[block_index] {
            return Ok(());
        }

        let partition = BloomPartition::new(self.hash_count);
        self.partitions.push(partition);
        self.loaded[block_index] = true;

        Ok(())
    }

    pub fn might_contain(&self, block_index: usize, key: &[u8]) -> Result<bool, BloomError> {
        if block_index >= self.max_partitions {
            return Err(BloomError::IndexOutOfBounds);
        }

        if !self.loaded[block_index] {
            return Err(BloomError::NotLoaded);
        }

        let partition = &self.partitions[block_index];
        Ok(partition.might_contain(key))
    }

    pub fn insert(&mut self, block_index: usize, key: &[u8]) -> Result<(), BloomError> {
        if block_index >= self.max_partitions {
            return Err(BloomError::IndexOutOfBounds);
        }

        if !self.loaded[block_index] {
            self.load_partition(block_index)?;
        }

        let partition = &mut self.partitions[block_index];
        partition.insert(key);

        Ok(())
    }

    pub fn is_full(&self) -> bool {
        self.partitions.len() >= self.max_partitions
    }

    pub fn memory_usage(&self) -> usize {
        self.partitions.len() * 8
    }
}

#[derive(Debug)]
pub enum BloomError {
    IndexOutOfBounds,
    NotLoaded,
    Full,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bloom_basic() {
        let mut filter = PartitionedBloom::new(10, 3);
        let key = b"hello";

        filter.insert(0, key).unwrap();
        assert!(filter.might_contain(0, key).unwrap());
    }

    #[test]
    fn test_bloom_false_positive_rate() {
        let mut filter = PartitionedBloom::new(50, 4);

        for i in 0..30 {
            let key = format!("key{}", i);
            filter.insert(i, key.as_bytes()).unwrap();
        }

        let mut false_positives = 0;
        for i in 30..50 {
            let key = format!("key{}", i);
            if let Ok(contains) = filter.might_contain(i, key.as_bytes()) {
                if contains {
                    false_positives += 1;
                }
            }
        }

        let fp_rate = false_positives as f32 / 20.0;
        println!("False positive rate: {:.2}%", fp_rate * 100.0);
        assert!(fp_rate < 0.05);
    }
}