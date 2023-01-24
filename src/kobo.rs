//! Types and functions for building and outputting a Kobo dictionary.

use std::collections::HashMap;
use std::io::prelude::*;
use std::io::BufWriter;
use std::path::Path;

use flate2::read::GzEncoder;
use unicode_categories::UnicodeCategories;

#[derive(Clone, Debug)]
pub struct Entry {
    // The integer here is a very rough priority ranking indicating
    // the commonness of the word, specifically in that form.  A
    // lower numerical value indicates a more common word.
    pub keys: Vec<(String, u32)>,
    pub definition: String,
}

pub fn write_dictionary(entries: &[Entry], output_path: &Path) -> std::io::Result<()> {
    // Sorted, de-duplicated list of keys.
    let all_keys = {
        let max_priority = entries
            .iter()
            .map(|e| &e.keys[..])
            .flatten()
            .fold(0u32, |a, b| a.max(b.1));
        let mut keys = HashMap::new();
        for entry in entries.iter() {
            for entry_key in entry.keys.iter() {
                let key = keys.entry(entry_key.0.clone()).or_insert(0);
                *key = (*key).max(max_priority - entry_key.1);
            }
        }
        let mut all_keys: Vec<(String, u32)> = keys.drain().collect();
        all_keys.sort_unstable();

        all_keys
    };

    //----------------------------------------------------------------
    // Create the `words` and `words.original` data.

    // Words as a new-line-separated text list.
    let words_original = {
        let mut words_original = String::new();
        for key in all_keys.iter() {
            words_original.push_str(&format!("{}\t{}\n", key.0, key.1));
        }
        words_original
    };

    // Create the marisa tree words data.
    let words = {
        // Write words to a temporary file.
        let mut words_file = tempfile::NamedTempFile::new().unwrap();
        words_file
            .as_file_mut()
            .write_all(words_original.as_bytes())
            .unwrap();
        words_file.as_file_mut().sync_all().unwrap();
        let words_path = words_file.into_temp_path();

        // Create a path for the marisa file.
        let mut marisa_path = words_path.to_path_buf();
        marisa_path.set_extension(".marisa.tmp");

        // Run marisa-build to create the marisa trie data.
        match std::process::Command::new("marisa-build")
            .arg("-o")
            .arg(marisa_path.as_os_str())
            .arg(words_path.as_os_str())
            .output()
        {
            Ok(output) => {
                if !output.status.success() {
                    eprintln!(
                        "Error: \"marisa-build\" exited with a failure:\n{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("Error: attempt to run \"marisa-build\" failed: {}", e);
                if e.kind() == std::io::ErrorKind::NotFound {
                    eprintln!("Make sure you have marisa-build installed and in your path, and that you have the permissions needed to run it.");
                }
                std::process::exit(1);
            }
        };

        // Read in the marisa file data.
        let mut data = Vec::new();
        let mut marisa_file = std::fs::File::open(&marisa_path).unwrap();
        marisa_file.read_to_end(&mut data).unwrap();

        data
    };

    //----------------------------------------------------------------
    // Duplicate the entries into a prefix list.

    // prefix -> Vec<(key, definition text, priority)>
    let mut prefix_entries: HashMap<String, Vec<(String, String, u32)>> = HashMap::new();

    for entry in entries.iter() {
        for key in entry.keys.iter() {
            let prefix = dictionary_prefix(&key.0);

            let a = prefix_entries.entry(prefix).or_insert(Vec::new());
            a.push((key.0.clone(), entry.definition.clone(), key.1));
        }
    }

    for entries in prefix_entries.values_mut() {
        // Sort by key, and then within key by priority, to prep for the
        // merging below.
        entries.sort_by_key(|a| (a.0.clone(), a.2));

        // Merge entries with the same key, so that Kobo e-readers show all
        // matches (their software is weird, and often omits duplicate exact
        // matches for some reason).
        let mut i = 0;
        while i < entries.len() {
            if i > 0 && entries[i].0 == entries[i - 1].0 {
                let entry = entries.remove(i);
                entries[i - 1].1.push_str(&entry.1);
                entries[i - 1].2 = entries[i - 1].2.min(entry.2);
            } else {
                i += 1;
            }
        }

        // Sort by priority, and then by inverse entry length, so
        // higher-priority and more detailed entries hopefully show
        // up first.
        entries.sort_by_key(|a| (a.2, -(a.1.len() as isize)));
    }

    //----------------------------------------------------------------
    // Write the Kobo dictionary file.

    // Open the output zip archive.
    let mut zip_out = zip::ZipWriter::new(BufWriter::new(std::fs::File::create(output_path)?));

    // Write the words and words.original files.
    zip_out
        .start_file("words", zip::write::FileOptions::default())
        .unwrap();
    zip_out.write_all(&words).unwrap();
    zip_out
        .start_file("words.original", zip::write::FileOptions::default())
        .unwrap();
    zip_out.write_all(words_original.as_bytes()).unwrap();

    // Write all of the prefix entry files.
    for (prefix, prefix_entry_list) in prefix_entries.iter() {
        // Generate the html.
        let mut html = String::new();
        html.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?><html>");
        for (key, definition, _) in prefix_entry_list.iter() {
            html.push_str(&format!(
                "<w><p><a name=\"{}\" />{}</p></w>",
                key, definition
            ));
        }
        html.push_str("</html>");

        // Compress with gzip.
        let mut gzhtml = Vec::new();
        let mut gz = GzEncoder::new(html.as_bytes(), flate2::Compression::fast());
        gz.read_to_end(&mut gzhtml).unwrap();

        // Write the file to the zip file.
        zip_out
            .start_file(
                &format!("{}.html", prefix),
                zip::write::FileOptions::default(),
            )
            .unwrap();
        zip_out.write_all(&gzhtml).unwrap();
    }

    zip_out.finish().unwrap();

    Ok(())
}

fn dictionary_prefix(key: &str) -> String {
    // See: https://pgaskin.net/dictutil/dicthtml/prefixes.html, which covers
    // the non-Japanese parts of this.

    // TODO: this totally punts on combining characters at the moment, and
    // therefore won't necessarily be correct for e.g. accented characters,
    // hangul, etc.  For Japanese, however, this should be fine.

    let prefix: Vec<_> = key.to_lowercase().trim().chars().take(2).collect();

    if prefix.is_empty() {
        return "11".into();
    }

    let ch = prefix[0] as u32;

    // Cyrillic and Japanese kana.
    if (ch >= 0x0400 && ch <= 0x052f)
        || (ch >= 0x2de0 && ch <= 0x2dff)
        || (ch >= 0xa640 && ch <= 0xa69f)
        || (ch >= 0x3040 && ch <= 0x30ff)
    {
        prefix.iter().collect()
    }
    // Basic Unicode plane Japanese Kanji / Chinese characters.
    else if (ch >= 0x3400 && ch <= 0x4dbf) || (ch >= 0x4e00 && ch <= 0x9fff) {
        prefix.iter().take(1).collect()
    }
    // Unicode letter class.
    else if prefix[0].is_letter() {
        if prefix.len() == 1 {
            [prefix[0], 'a'].iter().collect()
        } else if prefix[1].is_letter() {
            prefix.iter().collect()
        } else {
            "11".into()
        }
    }
    // For now, punt on everything else.
    else {
        "11".into()
    }
}
