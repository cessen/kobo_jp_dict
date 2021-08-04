//! Parses Yomichan .zip dictionaries.

use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;

use serde_json::Value;

//----------------------------------------------------------------
// Entry type for words.
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct TermEntry {
    pub dict_name: String,
    pub writing: String,
    pub reading: String,
    pub definitions: Vec<String>,
    pub infl: InflectionType,
    pub tags: Vec<String>,
    pub commonness: i32, // Higher is more common.
}

#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum InflectionType {
    VerbIchidan,
    VerbGodan,
    VerbSuru,
    VerbKuru,
    IAdjective,
    None,
}

//----------------------------------------------------------------
// Entry type for kanji.
#[derive(Clone, Debug)]
pub struct KanjiEntry {
    pub dict_name: String,
    pub kanji: String,
    pub onyomi: Vec<String>,
    pub kunyomi: Vec<String>,
    pub meanings: Vec<String>,
}

//----------------------------------------------------------------

pub fn parse(path: &Path) -> std::io::Result<(Vec<TermEntry>, Vec<TermEntry>, Vec<KanjiEntry>)> // (words, names, kanji)
{
    let mut zip_in = zip::ZipArchive::new(BufReader::new(File::open(path)?))?;

    let mut text = String::new();

    // Load index.json for meta-data about the dictionary.
    let index_json: Value = {
        text.clear();
        zip_in
            .by_name("index.json")
            .expect("Yomichan dictionary isn't valid: no index.json.")
            .read_to_string(&mut text)
            .expect("Yomichan dictionary isn't valid: invalid json.");
        serde_json::from_str(&text).expect("Yomichan dictionary isn't valid: invalid json.")
    };

    // Check the format version.
    match index_json.get("format") {
        Some(Value::Number(version)) if version.as_i64() == Some(3) => {}
        _ => panic!("Yomichan dictionaries other than format version 3 are not supported."),
    }

    // Get the normalized dictionary title.
    let dictionary_title: String = index_json
        .get("title")
        .expect("Yomichan dictionary isn't valid: index in unexpected format.")
        .as_str()
        .expect("Yomichan dictionary isn't valid: index in unexpected format.")
        .to_lowercase()
        .split("(")
        .nth(0)
        .unwrap()
        .trim()
        .into();

    // Is this a name dictionary?
    let is_name_dict = match dictionary_title.as_str() {
        "jmnedict" => true,
        _ => false,
    };

    // Loop through the bank-json files in the zip and build our entry list(s).
    let mut term_entries: HashMap<_, TermEntry> = HashMap::new();
    let mut name_entries = Vec::new();
    let mut kanji_entries = Vec::new();
    for i in 0..zip_in.len() {
        // Open the file.
        let mut f = zip_in.by_index(i).unwrap();
        let filename: String = std::str::from_utf8(f.name_raw()).unwrap().into();
        if !filename.ends_with(".json") {
            continue;
        }

        // Load the json data.
        text.clear();
        f.read_to_string(&mut text)
            .expect("Yomichan dictionary isn't valid: invalid json.");
        let json: Value =
            serde_json::from_str(&text).expect("Yomichan dictionary isn't valid: invalid json.");

        // Parse the json into entries.
        if filename.starts_with("term_bank_") {
            // It's a term bank.
            for item in json.as_array().unwrap().iter() {
                let mut tags: Vec<String> = item
                    .get(2)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .split(" ")
                    .chain(item.get(7).unwrap().as_str().unwrap().split(" "))
                    .map(|s| s.trim().into())
                    .filter(|s: &String| !s.is_empty())
                    .collect();
                tags.sort();
                tags.dedup();

                let mut entry = TermEntry {
                    dict_name: dictionary_title.clone(),
                    writing: item.get(0).unwrap().as_str().unwrap().trim().into(),
                    reading: item.get(1).unwrap().as_str().unwrap().trim().into(),
                    infl: match item.get(3).unwrap().as_str().unwrap().trim() {
                        "v1" => InflectionType::VerbIchidan,
                        "v5" => InflectionType::VerbGodan,
                        "vs" => InflectionType::VerbSuru,
                        "vk" => InflectionType::VerbKuru,
                        "adj-i" => InflectionType::IAdjective,
                        _ => InflectionType::None,
                    },
                    commonness: item.get(4).unwrap().as_i64().unwrap() as i32,
                    definitions: vec![item
                        .get(5)
                        .unwrap()
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|d| {
                            if let Some(s) = d.as_str() {
                                s.trim()
                            } else {
                                // Ignore the complex structured defintions for now.
                                // TODO: handle this properly.
                                ""
                            }
                        })
                        .collect::<Vec<&str>>()
                        .join("; ")],
                    tags: tags,
                };

                if is_name_dict {
                    name_entries.push(entry);
                } else {
                    // We do some extra work here to merge the definitions from
                    // multiple entries for the same word.
                    let key = (entry.writing.clone(), entry.reading.clone());
                    let e = term_entries.entry(key.clone()).or_insert(TermEntry {
                        dict_name: dictionary_title.clone(),
                        writing: entry.writing.clone(),
                        reading: entry.reading.clone(),
                        definitions: Vec::new(),
                        infl: entry.infl,
                        tags: Vec::new(),
                        commonness: entry.commonness,
                    });
                    e.definitions.extend(
                        entry
                            .definitions
                            .drain(..)
                            .filter(|d| {
                                // Attempt to get rid of English-Japanese definitions from
                                // native Japanese dictionaries.
                                !d.contains("英和") || key.0.contains("英和")
                            })
                            .map(|d| {
                                // Attempt to get rid of entry headers in the definitions.  They are
                                // annoyingly present in most of the native Japanese dictionaries
                                // converted to Yomichan format.
                                //
                                // Our heuristic is that if there's multiple lines, and the first
                                // line contains the Japanese word itself, then the first line is
                                // probably a header and we can drop it.
                                let header_indicator_idx =
                                    d.find(&key.0).or_else(|| d.find(&key.1));
                                let first_line_break_idx = d.find("\n");
                                match (header_indicator_idx, first_line_break_idx) {
                                    (Some(a), Some(b)) if a < b && (b + 1) < d.len() => {
                                        (&d[(b + 1)..]).into()
                                    }
                                    _ => d,
                                }
                            }),
                    );
                    e.tags.extend(entry.tags.drain(..));
                    e.tags.sort_unstable();
                    e.tags.dedup();
                }
            }
        } else if filename.starts_with("kanji_bank_") {
            // It's a kanji bank.
            for item in json.as_array().unwrap().iter() {
                let entry = KanjiEntry {
                    dict_name: dictionary_title.clone(),
                    kanji: item.get(0).unwrap().as_str().unwrap().trim().into(),
                    onyomi: item
                        .get(1)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .split(" ")
                        .map(|s| s.trim().into())
                        .filter(|s: &String| !s.is_empty())
                        .collect(),
                    kunyomi: item
                        .get(2)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .split(" ")
                        .map(|s| s.trim().into())
                        .filter(|s: &String| !s.is_empty())
                        .collect(),
                    meanings: item
                        .get(4)
                        .unwrap()
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|s| s.as_str().unwrap().trim().into())
                        .filter(|s: &String| !s.is_empty())
                        .collect(),
                };
                kanji_entries.push(entry);
            }
        }
    }

    // Convert the term entries into a simple `Vec`.
    let mut term_entries: Vec<TermEntry> = term_entries.drain().map(|kv| kv.1).collect();
    term_entries.sort_unstable();

    Ok((term_entries, name_entries, kanji_entries))
}
