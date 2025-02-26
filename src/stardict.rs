//! Types and functions for building and outputting a StarDict dictionary.

use std::io::prelude::*;
use std::io::BufWriter;
use std::path::Path;

use crate::generic_dict::Entry;

pub fn write_dictionary(entries: &[Entry], output_path: &Path) -> std::io::Result<()> {
    let dict_name = output_path.file_stem().unwrap().to_string_lossy();

    // Keys, sorted by string and then priority, with their priority and entry
    // index. (key, priority, entry_index)
    let keys: Vec<(String, u32, usize)> = {
        let max_priority = entries
            .iter()
            .map(|e| &e.keys[..])
            .flatten()
            .fold(0u32, |a, b| a.max(b.1));

        let mut keys: Vec<(String, u32, usize)> = Vec::new();

        for (i, entry) in entries.iter().enumerate() {
            for entry_key in entry.keys.iter() {
                keys.push((entry_key.0.clone(), max_priority - entry_key.1, i));
            }
        }

        keys.sort_unstable_by(|a, b| match stardict_strcmp(&a.0, &b.0) {
            std::cmp::Ordering::Equal => (a.1, a.2).cmp(&(b.1, b.2)),
            std::cmp::Ordering::Less => std::cmp::Ordering::Less,
            std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
        });

        keys
    };

    // For the .dict file.
    let (dict_data, dict_offset_len) = {
        let mut data = Vec::new();
        let mut offsets = Vec::new(); // (offset, len)

        for entry in entries {
            let start = data.len();
            data.extend(entry.definition.as_bytes());
            offsets.push((start, data.len() - start));
        }

        (data, offsets)
    };

    // For the .idx file.
    let (idx_data, idx_count) = {
        let mut data = Vec::new();
        let mut count = 0usize;

        for &(ref key, _, entry_idx) in &keys {
            if key.len() > 255 {
                continue;
            }
            let (offset, length) = dict_offset_len[entry_idx];
            data.extend(key.as_bytes());
            data.push(0); // Zero-terminated.
            data.extend(&(offset as u32).to_be_bytes());
            data.extend(&(length as u32).to_be_bytes());
            count += 1;
        }

        (data, count)
    };

    // For the .ifo file.
    let ifo_data: String = format!(
        "StarDict's dict ifo file
version=3.0.0
bookname={}
wordcount={}
idxfilesize={}
sametypesequence=h
lang=ja-en
",
        dict_name,
        idx_count,
        idx_data.len(),
    );

    //----------------------------------------------------------------
    // Write a zip file with all the files for the dictionary in it.

    // Open the output zip archive.
    let mut zip_out = zip::ZipWriter::new(BufWriter::new(std::fs::File::create(output_path)?));

    let base_path = format!("{}/{}", dict_name, dict_name);

    // Dict file.
    let dict_filepath = format!("{}.dict", base_path);
    zip_out
        .start_file(&dict_filepath, zip::write::FileOptions::default())
        .unwrap();
    zip_out.write_all(&dict_data).unwrap();

    // Idx file.
    let idx_filepath = format!("{}.idx", base_path);
    zip_out
        .start_file(&idx_filepath, zip::write::FileOptions::default())
        .unwrap();
    zip_out.write_all(&idx_data).unwrap();

    // Ifo file.
    let ifo_filepath = format!("{}.ifo", base_path);
    zip_out
        .start_file(&ifo_filepath, zip::write::FileOptions::default())
        .unwrap();
    zip_out.write_all(ifo_data.as_bytes()).unwrap();

    zip_out.finish().unwrap();
    Ok(())
}

/// A StarDict-compatible string comparison function for use in sorting strings.
fn stardict_strcmp(a: &str, b: &str) -> std::cmp::Ordering {
    // TODO: if this ends up being a performance bottleneck for the sorting,
    // rewrite to avoid the heap allocation.
    let a_lower = a.to_ascii_lowercase();
    let b_lower = b.to_ascii_lowercase();

    match a_lower.cmp(&b_lower) {
        std::cmp::Ordering::Equal => a.cmp(b),
        std::cmp::Ordering::Less => std::cmp::Ordering::Less,
        std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
    }
}
