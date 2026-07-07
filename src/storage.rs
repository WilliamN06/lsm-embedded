use core::fmt::Debug;

pub trait Storage {
    type Error: Debug + 'static + From<StorageError>;

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<(), Self::Error>;
    fn read_at(&mut self, offset: u64, data: &mut [u8]) -> Result<(), Self::Error>;
    fn sync(&mut self) -> Result<(), Self::Error>;
    fn capacity(&self) -> u64;
}

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

#[cfg(feature = "std")]
impl From<StorageError> for std::io::Error {
    fn from(err: StorageError) -> Self {
        use std::io::ErrorKind;
        match err {
            StorageError::Io => ErrorKind::Other.into(),
            StorageError::Corruption => ErrorKind::InvalidData.into(),
            StorageError::Full => ErrorKind::StorageFull.into(),
            StorageError::NotFound => ErrorKind::NotFound.into(),
        }
    }
}

#[cfg(feature = "std")]
pub struct InMemoryStorage {
    data: std::collections::BTreeMap<u64, u8>,
    capacity: u64,
}

#[cfg(feature = "std")]
impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            data: std::collections::BTreeMap::new(),
            capacity: 1024 * 1024 * 100,
        }
    }

    pub fn with_capacity(capacity: u64) -> Self {
        Self {
            data: std::collections::BTreeMap::new(),
            capacity,
        }
    }
}

#[cfg(feature = "std")]
impl Storage for InMemoryStorage {
    type Error = std::io::Error;

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        for (i, byte) in data.iter().enumerate() {
            self.data.insert(offset + i as u64, *byte);
        }
        Ok(())
    }

    fn read_at(&mut self, offset: u64, data: &mut [u8]) -> Result<(), Self::Error> {
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = *self.data.get(&(offset + i as u64)).unwrap_or(&0);
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn capacity(&self) -> u64 {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_storage() {
        let mut storage = InMemoryStorage::new();
        let data = [1, 2, 3, 4, 5];
        
        storage.write_at(10, &data).unwrap();
        
        let mut buf = [0u8; 5];
        storage.read_at(10, &mut buf).unwrap();
        assert_eq!(buf, data);
        
        let mut buf2 = [0u8; 3];
        storage.read_at(15, &mut buf2).unwrap();
        assert_eq!(buf2, [0, 0, 0]);
    }
}