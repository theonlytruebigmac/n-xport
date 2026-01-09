//! JSON export functionality

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use serde::Serialize;

use crate::error::{AppError, Result};

/// Export data to JSON file (pretty-printed)
pub fn export_to_json<T, P>(data: &[T], path: P) -> Result<usize>
where
    T: Serialize,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let count = data.len();
    
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let writer = BufWriter::new(file);

    serde_json::to_writer_pretty(writer, data)
        .map_err(|e| AppError::Export(format!("JSON serialization error: {}", e)))?;

    Ok(count)
}

/// Export data to JSON file (compact)
pub fn export_to_json_compact<T, P>(data: &[T], path: P) -> Result<usize>
where
    T: Serialize,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let count = data.len();
    
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let writer = BufWriter::new(file);

    serde_json::to_writer(writer, data)
        .map_err(|e| AppError::Export(format!("JSON serialization error: {}", e)))?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use tempfile::TempDir;

    #[derive(Serialize)]
    struct TestData {
        id: i32,
        name: String,
    }

    #[test]
    fn test_export_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.json");

        let data = vec![
            TestData { id: 1, name: "Test 1".into() },
            TestData { id: 2, name: "Test 2".into() },
        ];

        let count = export_to_json(&data, &path).unwrap();
        assert_eq!(count, 2);
        assert!(path.exists());
    }
}
