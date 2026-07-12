use crate::storage::Storage;
use crate::StorageError;
use core::fmt;

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
use wasm_bindgen::prelude::*;

pub struct WasmStorage {
    memory: Vec<u8>,
    capacity: u64,
    position: u64,
}

impl WasmStorage {
    pub fn new() -> Self {
        Self {
            memory: Vec::new(),
            capacity: 1024 * 1024 * 10,
            position: 0,
        }
    }

    pub fn with_capacity(capacity: u64) -> Self {
        Self {
            memory: Vec::with_capacity(capacity as usize),
            capacity,
            position: 0,
        }
    }

    pub fn data(&self) -> &[u8] {
        &self.memory
    }
}

impl Storage for WasmStorage {
    type Error = WasmStorageError;

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<(), Self::Error> {
        let start = offset as usize;
        let end = start + data.len();

        if end > self.memory.len() {
            self.memory.resize(end, 0);
        }

        self.memory[start..end].copy_from_slice(data);
        self.position = end as u64;
        Ok(())
    }

    fn read_at(&mut self, offset: u64, data: &mut [u8]) -> Result<(), Self::Error> {
        let start = offset as usize;
        let end = start + data.len();

        if end > self.memory.len() {
            return Err(WasmStorageError::OutOfBounds);
        }

        data.copy_from_slice(&self.memory[start..end]);
        self.position = end as u64;
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn capacity(&self) -> u64 {
        self.capacity
    }
}

#[derive(Debug, PartialEq)]
pub enum WasmStorageError {
    OutOfBounds,
    Io,
    Corruption,
    Full,
    NotFound,
}

impl From<StorageError> for WasmStorageError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::Io => WasmStorageError::Io,
            StorageError::Corruption => WasmStorageError::Corruption,
            StorageError::Full => WasmStorageError::Full,
            StorageError::NotFound => WasmStorageError::NotFound,
        }
    }
}

impl fmt::Display for WasmStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WasmStorageError::OutOfBounds => write!(f, "Write out of bounds"),
            WasmStorageError::Io => write!(f, "I/O error"),
            WasmStorageError::Corruption => write!(f, "Data corruption"),
            WasmStorageError::Full => write!(f, "Storage full"),
            WasmStorageError::NotFound => write!(f, "Data not found"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for WasmStorageError {}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
#[wasm_bindgen]
pub struct WasmStorageHandle {
    storage: WasmStorage,
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
#[wasm_bindgen]
impl WasmStorageHandle {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            storage: WasmStorage::new(),
        }
    }

    #[wasm_bindgen]
    pub fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), JsValue> {
        self.storage.write_at(offset, data)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[wasm_bindgen]
    pub fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, JsValue> {
        let mut buf = vec![0u8; len];
        self.storage.read_at(offset, &mut buf)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(buf)
    }

    #[wasm_bindgen]
    pub fn capacity(&self) -> u64 {
        self.storage.capacity()
    }

    #[wasm_bindgen]
    pub fn data(&self) -> Vec<u8> {
        self.storage.data().to_vec()
    }
}