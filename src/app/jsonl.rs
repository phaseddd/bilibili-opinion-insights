use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::hash::Hash;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) fn append_jsonl_writer(path: &Path) -> Result<BufWriter<File>> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open JSONL output for append: {}", path.display()))?;
    Ok(BufWriter::new(file))
}

pub(crate) fn write_jsonl_record<T: Serialize>(
    writer: &mut BufWriter<File>,
    record: &T,
) -> Result<()> {
    serde_json::to_writer(&mut *writer, record)?;
    writeln!(writer)?;
    Ok(())
}

pub(crate) fn flush_jsonl_writer(writer: &mut BufWriter<File>) -> Result<()> {
    writer.flush()?;
    writer.get_mut().sync_data()?;
    Ok(())
}

pub(crate) fn read_jsonl_records<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = File::open(path)
        .with_context(|| format!("failed to open JSONL input: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "failed to read JSONL line {} from {}",
                index + 1,
                path.display()
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str(&line).with_context(|| {
            format!(
                "failed to parse JSONL line {} from {}",
                index + 1,
                path.display()
            )
        })?);
    }

    Ok(records)
}

pub(crate) fn read_jsonl_keys<K, F>(path: &Path, mut key_from_value: F) -> Result<HashSet<K>>
where
    K: Eq + Hash,
    F: FnMut(&serde_json::Value) -> Option<K>,
{
    if !path.exists() {
        return Ok(HashSet::new());
    }

    let file = File::open(path)
        .with_context(|| format!("failed to open JSONL input: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut keys = HashSet::new();

    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "failed to read JSONL line {} from {}",
                index + 1,
                path.display()
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line).with_context(|| {
            format!(
                "failed to parse JSONL line {} from {}",
                index + 1,
                path.display()
            )
        })?;
        if let Some(key) = key_from_value(&value) {
            keys.insert(key);
        }
    }

    Ok(keys)
}
