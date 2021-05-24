//! Parses the Kobo's Japanse-Japanese dictionary.

use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;

use flate2::read::GzDecoder;
use quick_xml::events::Event;

use crate::{hiragana_to_katakana, strip_non_kana};

#[derive(Clone, Debug)]
pub struct Entry {
    pub key: String,
    pub writings: Vec<String>,
    pub kana: String,
    pub definition: String,
}

pub fn parse(path: &Path, print_progress: bool) -> std::io::Result<Vec<Entry>> {
    let mut zip_in = zip::ZipArchive::new(BufReader::new(File::open(path)?))?;
    let re_writings = regex::Regex::new(r"【([^】]*)】").unwrap();

    let mut entry_list = Vec::new();
    let mut data = Vec::new();
    let mut html = String::new();
    for i in 0..zip_in.len() {
        if print_progress {
            print!("\rLoading Kobo dictionary: {}/{}", i + 1, zip_in.len());
        }

        let mut f = zip_in.by_index(i).unwrap();

        // Skip if it's not one of the html files.
        let name_raw = f.name_raw();
        if !(name_raw.len() >= 5 && &name_raw[(name_raw.len() - 5)..] == &b".html"[..]) {
            continue;
        }

        // Get the unzipped, un-gzipped html data for this file.
        data.clear();
        html.clear();
        f.read_to_end(&mut data).unwrap();
        let mut ungz = GzDecoder::new(&data[..]);
        ungz.read_to_string(&mut html)?;

        // Parse the file, adding entries to the entry list as we go.
        //
        // Note: we leave out images for a couple of reasons:
        // 1. We don't transfer the images over, so they become broken
        //    links anyway.
        // 2. Including too many (broken?) image tags causes Kobo to crash
        //    if they're in one of its dictionaries.
        let mut parser = quick_xml::Reader::from_str(&html);
        let mut state = PS::None;
        let mut buf = Vec::new();
        let mut definition = String::new();
        while let Ok(event) = parser.read_event(&mut buf) {
            match event {
                Event::Eof => {
                    break;
                }

                Event::Start(e) => {
                    // Leave out images.
                    if e.name() == b"img" {
                        continue;
                    }
                    // Are we in the middle of collecting the word header info?
                    else if let PS::Word(word) = state.clone() {
                        if e.name() == b"b" {
                            // Get the kana pronunciation.
                            let kana = if let Ok(Event::Text(e)) = parser.read_event(&mut buf) {
                                strip_non_kana(&hiragana_to_katakana(bytes_to_str(&e).trim()))
                            } else {
                                "".into()
                            };
                            let _ = parser.read_event(&mut buf); // Skip "</b>".

                            // Get the (probably kanji) writings.
                            let mut writings = Vec::new();
                            if let Ok(Event::Text(e)) = parser.read_event(&mut buf) {
                                let text = bytes_to_str(&e);
                                if let Some(cap) = re_writings.captures_iter(text).next() {
                                    let tmp: Vec<_> =
                                        cap[1].split("／").map(|s| s.into()).collect();
                                    writings.extend_from_slice(&tmp);
                                }
                            }
                            if !writings.contains(&word) {
                                writings.push(word.clone());
                            }

                            state = PS::NeedDefinition {
                                key: word,
                                kana: kana,
                                writings: writings,
                            };
                        }
                    }
                    // Are we in the middle of collecting the definition?
                    else if let PS::NeedDefinition { .. } = state {
                        // Copy to the definition.
                        definition.push_str(&format!("<{}>", bytes_to_str(&e)));
                    }
                }

                Event::Empty(e) => {
                    // Leave out images.
                    if e.name() == b"img" {
                        continue;
                    }
                    // Did we find the start of a new entry?
                    else if state == PS::None
                        && e.name() == b"a"
                        && e.attributes().count() > 0
                        && e.attributes().nth(0).unwrap().unwrap().key == b"name"
                    {
                        definition.clear();
                        state = PS::Word(bytes_to_string(
                            &e.attributes().nth(0).unwrap().unwrap().value,
                        ));
                    }
                    // Are we in the middle of collecting the definition?
                    else if let PS::NeedDefinition { .. } = state {
                        // Copy to the definition.
                        definition.push_str(&format!("<{}/>", bytes_to_str(&e)));
                    }
                }

                Event::End(e) => {
                    // Leave out images.
                    if e.name() == b"img" {
                        continue;
                    }
                    // Is it the end of an entry?
                    else if e.name() == b"w" {
                        if let PS::NeedDefinition {
                            key,
                            kana,
                            writings,
                        } = state
                        {
                            // Get rid of the extra "</p>" at the end of the definition.
                            definition.pop();
                            definition.pop();
                            definition.pop();
                            definition.pop();

                            entry_list.push(Entry {
                                key: key,
                                writings: writings,
                                kana: kana,
                                definition: definition.clone(),
                            });

                            definition.clear();
                        }

                        state = PS::None;
                    }
                    // Are we in the middle of collecting the definition?
                    else if let PS::NeedDefinition { .. } = state {
                        // Copy to the definition.
                        definition.push_str(&format!("</{}>", bytes_to_str(&e)));
                    }
                }

                Event::Text(e) => {
                    let text = bytes_to_str(&e);

                    // Are we in the middle of collecting the definition?
                    if let PS::NeedDefinition { .. } = state {
                        // Copy to the definition.
                        definition.push_str(text);
                    }
                }

                _ => {}
            }
        }
    }
    println!();

    Ok(entry_list)
}

/// Parse State (PS)
#[derive(Clone, Debug, PartialEq, Eq)]
enum PS {
    None,
    Word(String),
    NeedDefinition {
        key: String,
        kana: String,
        writings: Vec<String>,
    },
}

/// Panics if the bytes aren't utf8.
fn bytes_to_string(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes).unwrap().into()
}

/// Panics if the bytes aren't utf8.
fn bytes_to_str(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}
