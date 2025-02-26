//! Parses Yomichan .zip dictionaries.

use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;

use furigana_gen::FuriganaGenerator;
use regex::Regex;
use serde_json::Value;

//----------------------------------------------------------------
// Entry type for words.
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct TermEntry {
    pub dict_name: String,
    pub writing: String,
    pub reading: String,
    pub definitions: Definition,
    pub infl: InflectionType,
    pub tags: Vec<String>,
    pub commonness: i32, // Higher is more common.
}

// A (possibly hierarchical) list of definitions.
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub enum Definition {
    List((String, Vec<Definition>)), // (header, list)
    Def(String),
}

impl Definition {
    pub fn new() -> Definition {
        Definition::List(("".into(), Vec::new()))
    }

    pub fn is_list(&self) -> bool {
        match self {
            &Definition::List(_) => true,
            &Definition::Def(_) => false,
        }
    }

    pub fn depth(&self) -> usize {
        match self {
            &Definition::List((_, ref l)) => 1 + l.iter().fold(0usize, |a, b| a.max(b.depth())),
            &Definition::Def(_) => 0,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            &Definition::List((_, ref l)) => l.len(),
            &Definition::Def(_) => 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            &Definition::List((ref h, ref l)) => h.trim().is_empty() && l.is_empty(),
            &Definition::Def(ref s) => s.trim().is_empty(),
        }
    }

    pub fn def_text(&self) -> &str {
        if let &Definition::Def(ref text) = self {
            text
        } else {
            panic!("Definition is a list, cannot fetch text.")
        }
    }
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

pub fn parse(
    path: &Path,
    furigana_generator: Option<&FuriganaGenerator>,
) -> std::io::Result<(Vec<TermEntry>, Vec<TermEntry>, Vec<KanjiEntry>)> // (words, names, kanji)
{
    let mut furigen = furigana_generator.map(|fg| fg.new_session(false));

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

            // Dividers for the 三省堂　スーパー大辞林 dictionary.
            // But probably works for some other native Japanese
            // dictionaries as well.
            let dividers = &[
                // The (?m) puts the regex into multi-line mode, so
                // that ^ will match both newlines and start of text.
                Regex::new("(?m)^■[一二三四五六七八九十]+■").unwrap(),
                Regex::new("(?m)^[❶❷❸❹❺❻❼❽❾❿]+").unwrap(),
                Regex::new("(?m)^（[０１２３４５６７８９]+）").unwrap(),
            ];

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
                    definitions: Definition::List((
                        "".into(),
                        vec![Definition::Def(
                            item.get(5)
                                .unwrap()
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|d| {
                                    if let Some(s) = d.as_str() {
                                        s.trim()
                                    } else {
                                        // Ignore the complex structured definitions for now.
                                        // TODO: handle this properly.
                                        ""
                                    }
                                })
                                .collect::<Vec<&str>>()
                                .join("; "),
                        )],
                    )),
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
                        definitions: Definition::List(("".into(), Vec::new())),
                        infl: entry.infl,
                        tags: Vec::new(),
                        commonness: entry.commonness,
                    });
                    assert!(e.definitions.is_list());
                    if let Definition::List((_, ref mut list_to)) = e.definitions {
                        match entry.definitions {
                            Definition::List((_, mut list_from)) => {
                                list_to.extend(list_from.drain(..).filter_map(|d| {
                                    process_definition(&key.0, &key.1, dividers, d, &mut furigen)
                                }))
                            }
                            Definition::Def(s) => list_to.push(Definition::Def(s)),
                        }
                    }
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

/// Recursively process definitions.
///
/// The `dividers` regex's are for further splitting definitions into a
/// deeper hierarchy.  The first regex in the list is used for the top
/// level split, the second for the second level, and so on.
fn process_definition(
    writing: &str,
    reading: &str,
    dividers: &[Regex],
    def: Definition,
    furigen: &mut Option<furigana_gen::Session>,
) -> Option<Definition> {
    match def {
        Definition::List((header, mut list)) => {
            let mut processed_list: Vec<_> = list
                .drain(..)
                .filter_map(|d| process_definition(writing, reading, dividers, d, furigen))
                .collect();
            if processed_list.is_empty() {
                None
            } else if header.trim().is_empty() && processed_list.len() == 1 {
                Some(processed_list.remove(0))
            } else {
                Some(Definition::List((header, processed_list)))
            }
        }

        Definition::Def(mut s) => {
            // Attempt to get rid of English-Japanese definitions from
            // native Japanese dictionaries.
            if s.contains("英和") && !writing.contains("英和") {
                return None;
            }

            // Attempt to get rid of entry headers in the definitions.  They are
            // annoyingly present in most of the native Japanese dictionaries
            // converted to Yomichan format.
            //
            // Our heuristic is that if there's multiple lines, and the first
            // line contains the Japanese word itself, then the first line is
            // probably a header and we can drop it.
            s = {
                let header_indicator_idx = s.find(writing).or_else(|| s.find(reading));
                let first_line_break_idx = s.find("\n");
                match (header_indicator_idx, first_line_break_idx) {
                    (Some(a), Some(b)) if a < b && (b + 1) < s.len() => (&s[(b + 1)..]).into(),
                    _ => s,
                }
            };

            // Guess if it's an English or Japanese definition.
            let is_english = {
                let mut ascii_count = 0;
                let mut total_count = 0;
                for c in s.chars() {
                    total_count += 1;
                    if c.is_ascii() {
                        ascii_count += 1;
                    }
                }
                (ascii_count as f64 / total_count as f64) > 0.5
            };

            // Add furigana if it's not English.
            if is_english {
                Some(split_definition_text(&s, dividers, &mut None))
            } else {
                Some(split_definition_text(&s, dividers, furigen))
            }
        }
    }
}

fn split_definition_text(
    s: &str,
    dividers: &[Regex],
    furigen: &mut Option<furigana_gen::Session>,
) -> Definition {
    // Try each divider in turn, to divide into sub-definitions.
    for divider in dividers.iter() {
        let match_count = divider.find_iter(&s).count();
        if match_count > 0 {
            let mut list: Vec<Definition> = divider
                .split(&s)
                .filter(|t| !t.trim().is_empty())
                .map(|t| split_definition_text(t.into(), dividers, furigen))
                .collect();

            if list.is_empty() {
                break;
            } else if list.len() == 1 {
                return list.remove(0);
            } else {
                let header = if list.len() > match_count && !list[0].is_list() {
                    let tmp = list.remove(0);
                    tmp.def_text().into()
                } else {
                    String::new()
                };
                return Definition::List((header, list));
            }
        }
    }

    // If none of the dividers matched, just return the text as-is.
    if let Some(ref mut furigen) = furigen {
        Definition::Def(furigen.add_html_furigana(&s.trim().replace("\n", "<br>")))
    } else {
        Definition::Def(s.trim().replace("\n", "<br>"))
    }
}

/// Converts a defintion(s) to html.
///
/// `ordered_list` is whether to use an ordered html list type or
/// unordered.
pub fn definition_to_html(def: &Definition, total_depth: usize, ordered_list: bool) -> String {
    let mut html = String::new();
    let depth = def.depth();

    match def {
        &Definition::List((ref header, ref list)) => {
            if header.trim().is_empty() && list.len() == 1 {
                return definition_to_html(&list[0], total_depth - 1, ordered_list);
            } else {
                if !header.trim().is_empty() {
                    html.push_str("<p>");
                    html.push_str(header.trim());
                    html.push_str("</p>");
                }
                if ordered_list {
                    match total_depth {
                        0 => panic!("Incorrect `total_depth` given."),
                        1 => match total_depth.saturating_sub(depth) {
                            0 => html.push_str("<ol style=\"list-style-type: decimal\">"),
                            _ => panic!("Incorrect `total_depth` given."),
                        },
                        2 => match total_depth.saturating_sub(depth) {
                            0 => html.push_str("<ol style=\"list-style-type: upper-roman\">"),
                            1 => html.push_str("<ol style=\"list-style-type: decimal\">"),
                            _ => panic!("Incorrect `total_depth` given."),
                        },
                        _ => match total_depth.saturating_sub(depth) {
                            0 => html.push_str("<ol style=\"list-style-type: upper-roman\">"),
                            1 => html.push_str("<ol style=\"list-style-type: upper-alpha\">"),
                            2 => html.push_str("<ol style=\"list-style-type: decimal\">"),
                            _ => html.push_str("<ol style=\"list-style-type: decimal\">"),
                        },
                    }
                } else {
                    html.push_str("<ul>");
                }
                for d in list.iter() {
                    html.push_str("<li>");
                    html.push_str(&definition_to_html(d, total_depth, ordered_list));
                    html.push_str("</li>");
                }
                if ordered_list {
                    html.push_str("</ol>");
                } else {
                    html.push_str("</ul>");
                }
            }
        }

        &Definition::Def(ref s) => {
            if total_depth == 0 {
                if ordered_list {
                    html.push_str("<ol><li>");
                    html.push_str(s);
                    html.push_str("</li></ol>");
                } else {
                    html.push_str("<ul><li>");
                    html.push_str(s);
                    html.push_str("</li></ul>");
                }
            } else {
                html.push_str(s);
            }
        }
    }

    html
}
