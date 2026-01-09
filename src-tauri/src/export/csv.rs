//! CSV export functionality

use std::fs::File;
use std::path::Path;
use serde::Serialize;

use crate::error::{AppError, Result};

/// Export data to CSV file
pub fn export_to_csv<T, P>(data: &[T], path: P) -> Result<usize>
where
    T: Serialize,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let mut writer = csv::Writer::from_writer(file);

    let mut count = 0;
    for item in data {
        writer.serialize(item)
            .map_err(|e| AppError::Export(format!("CSV serialization error: {}", e)))?;
        count += 1;
    }

    writer.flush()
        .map_err(|e| AppError::Export(format!("CSV flush error: {}", e)))?;

    Ok(count)
}

/// Export data to CSV file with progress callback
pub fn export_to_csv_with_progress<T, P, F>(
    data: &[T],
    path: P,
    mut on_progress: F,
) -> Result<usize>
where
    T: Serialize,
    P: AsRef<Path>,
    F: FnMut(usize, usize),
{
    let path = path.as_ref();
    let total = data.len();
    
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let mut writer = csv::Writer::from_writer(file);

    for (i, item) in data.iter().enumerate() {
        writer.serialize(item)
            .map_err(|e| AppError::Export(format!("CSV serialization error: {}", e)))?;
        
        // Report progress every 100 items
        if i % 100 == 0 || i == total - 1 {
            on_progress(i + 1, total);
        }
    }

    writer.flush()
        .map_err(|e| AppError::Export(format!("CSV flush error: {}", e)))?;

    Ok(total)
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
    fn test_export_csv() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.csv");

        let data = vec![
            TestData { id: 1, name: "Test 1".into() },
            TestData { id: 2, name: "Test 2".into() },
        ];

        let count = export_to_csv(&data, &path).unwrap();
        assert_eq!(count, 2);
        assert!(path.exists());
    }
}
