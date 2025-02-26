#![allow(dead_code)]

#[macro_use]
extern crate lazy_static;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;

use flate2::read::GzDecoder;
use furigana_gen::FuriganaGenerator;

mod generic_dict;
mod jmdict;
mod kobo;
mod yomichan;

use generic_dict::LangMode;
use jmdict::WordEntry;

fn main() -> io::Result<()> {
    let matches = clap::Command::new("Kobo Japanese Dictionary Builder")
        .version(clap::crate_version!())
        .arg(
            clap::Arg::new("OUTPUT")
                .help("The output filepath to write the new dictionary to.")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::new("pitch_accent")
                .short('p')
                .long("pitch_accent")
                .help("Path to a custom pitch accent file in .tsv format.  Will be used instead of the bundled pitch accent data.")
                .value_name("PATH")
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("yomichan_dict")
                .short('y')
                .long("yomichan")
                .help("Path to a zipped Yomichan dictionary.  Will add either additional definitions to existing entries or completely new entries, depending the dictionary.")
                .value_name("PATH")
                .takes_value(true)
                .multiple_occurrences(true),
        )
        .arg(
            clap::Arg::new("katakana_pronunciation")
                .short('k')
                .long("katakana")
                .help("Use katakana instead of hiragana for word pronunciation."),
        )
        .arg(
            clap::Arg::new("use_move_terms")
                .short('m')
                .long("use_move_terms")
                .help("Use the terms \"other-move\" and \"self-move\" instead of \"transitive\" and \"intransitive\".  The former is more accurate to how Japanese works, but the latter are more commonly known and used."),
        )
        .arg(
            clap::Arg::new("use_japanese_terms")
                .short('j')
                .long("use_japanese_terms")
                .help("Use the Japanese terms for \"verb\", \"transitive\", etc. instead of English in entry headers."),
        )
        .arg(
            clap::Arg::new("generate_furigana")
                .short('f')
                .long("generate_furigana")
                .help("Auto-generate furigana on native Japanese definitions."),
        )
        .get_matches();

    // Output zip archive path.
    let output_filename = matches.value_of("OUTPUT").unwrap();

    //----------------------------------------------------------------
    // Read in all the files.

    println!("Extracting bundled data...");

    // Parse the bundled JMDict XML data.
    const JM_DATA: &[u8] = include_bytes!("../dictionaries/JMdict_e.xml.gz");
    let jm_table = {
        let mut jm_table: HashMap<(String, String), Vec<WordEntry>> = HashMap::new(); // (Kanji, Kana)
        let parser = jmdict::Parser::from_reader(BufReader::new(GzDecoder::new(JM_DATA)));
        for entry in parser {
            let reading = strip_non_kana(&hiragana_to_katakana(&entry.readings[0].trim()));
            let writing = if entry.writings.len() > 0 {
                entry.writings[0].clone()
            } else {
                entry.readings[0].trim().into()
            };

            let e = jm_table.entry((writing, reading)).or_insert(Vec::new());
            e.push(entry);
        }
        jm_table
    };
    println!("    Metadata entries: {}", jm_table.len());

    // Open and parse the pitch accent data.
    const PA_DATA: &[u8] = include_bytes!("../dictionaries/accents.tsv.gz");
    let pa_table = {
        let mut pa_table: HashMap<(String, String), Vec<u32>> = HashMap::new(); // (Kanji, Kana), Pitch Accent

        // Use the passed file if specified on the command line.  Otherwise use the bundled one.
        let mut data = Vec::new();
        if let Some(path) = matches.value_of("pitch_accent") {
            File::open(path)?.read_to_end(&mut data)?;
        } else {
            GzDecoder::new(PA_DATA).read_to_end(&mut data)?;
        };
        let reader = std::io::Cursor::new(data);

        for line in reader.lines() {
            let line = line.unwrap_or_else(|_| "".into());
            let parts: Vec<_> = line.split("\t").map(|a| a.trim()).collect();
            assert_eq!(parts.len(), 3);
            let accents: Vec<u32> = parts[2]
                .split(|ch: char| !ch.is_digit(10))
                .filter(|s| !s.is_empty())
                .map(|a| a.parse::<u32>().unwrap())
                .collect();

            let (writing, reading) = if is_all_kana(parts[0]) && parts[1].is_empty() {
                (parts[0].into(), hiragana_to_katakana(parts[0]))
            } else {
                (parts[0].into(), hiragana_to_katakana(parts[1]))
            };

            pa_table.insert((writing, reading), accents);
        }
        pa_table
    };
    println!("    Pitch Accent entries: {}", pa_table.len());

    println!("Loading dictionaries...");

    // For auto-adding furigana to native Japanese dictionary entries.
    let furigana_generator = if matches.is_present("generate_furigana") {
        Some(FuriganaGenerator::new(0, true, false))
    } else {
        None
    };

    // Open and parse Yomichan dictionaries.
    let mut yomi_term_table: HashMap<(String, String), Vec<yomichan::TermEntry>> = HashMap::new(); // (Kanji, Kana)
    let mut yomi_name_table: HashMap<(String, String), Vec<yomichan::TermEntry>> = HashMap::new(); // (Kanji, Kana)
    let mut yomi_kanji_table: HashMap<String, Vec<yomichan::KanjiEntry>> = HashMap::new(); // Kanji
    if let Some(paths) = matches.values_of("yomichan_dict") {
        for path in paths {
            let mut entry_count = 0usize;

            let (mut word_entries, mut name_entries, mut kanji_entries) =
                yomichan::parse(std::path::Path::new(path), furigana_generator.as_ref()).unwrap();

            // Put all of the word entries into the terms table.
            entry_count += word_entries.len();
            for entry in word_entries.drain(..) {
                let reading = strip_non_kana(&hiragana_to_katakana(entry.reading.trim()));
                let writing: String = entry.writing.trim().into();
                if writing.is_empty() {
                    let entry_list = yomi_term_table
                        .entry((entry.reading.trim().into(), reading))
                        .or_insert(Vec::new());
                    entry_list.push(entry);
                } else if reading.is_empty() && is_all_kana(&writing) {
                    let derived_reading = hiragana_to_katakana(&writing);
                    let entry_list = yomi_term_table
                        .entry((writing, derived_reading))
                        .or_insert(Vec::new());
                    entry_list.push(entry);
                } else {
                    let entry_list = yomi_term_table
                        .entry((writing, reading))
                        .or_insert(Vec::new());
                    entry_list.push(entry);
                }
            }

            // Put all of the name entries into the names table.
            entry_count += name_entries.len();
            for entry in name_entries.drain(..) {
                let reading = strip_non_kana(&hiragana_to_katakana(entry.reading.trim()));
                let writing: String = entry.writing.trim().into();
                if writing.is_empty() {
                    let entry_list = yomi_name_table
                        .entry((entry.reading.trim().into(), reading))
                        .or_insert(Vec::new());
                    entry_list.push(entry);
                } else {
                    let entry_list = yomi_name_table
                        .entry((writing, reading))
                        .or_insert(Vec::new());
                    entry_list.push(entry);
                }
            }

            // Put all of the kanji entries into the kanji table.
            entry_count += kanji_entries.len();
            for entry in kanji_entries.drain(..) {
                let entry_list = yomi_kanji_table
                    .entry(entry.kanji.clone())
                    .or_insert(Vec::new());
                entry_list.push(entry);
            }

            println!("    {} entries: {}", path, entry_count);
        }
    }

    //----------------------------------------------------------------
    // Generate the new dictionary entries.
    let entries = generic_dict::generate_entries(
        &yomi_term_table,
        &yomi_name_table,
        &yomi_kanji_table,
        &jm_table,
        &pa_table,
        generic_dict::EntrySettings {
            lang_mode: if matches.is_present("use_japanese_terms") {
                LangMode::Japanese
            } else if matches.is_present("use_move_terms") {
                LangMode::EnglishAlt
            } else {
                LangMode::English
            },
            use_katakana_pronunciation: matches.is_present("katakana_pronunciation"),
            generate_inflection_keys: true,
        },
    );

    //----------------------------------------------------------------
    // Write the new dictionary file.
    println!("Writing dictionary to disk...");
    kobo::write_dictionary(&entries, std::path::Path::new(output_filename))?;

    return Ok(());
}

/// Panics if the bytes aren't utf8.
fn bytes_to_string(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes).unwrap().into()
}

/// Panics if the bytes aren't utf8.
fn bytes_to_str(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

/// Numerical difference between hiragana and katakana in scalar values.
/// Hirgana is lower than katakana.
const KANA_DIFF: u32 = 0x30a1 - 0x3041;

fn is_kana(ch: char) -> bool {
    let c = ch as u32;

    (c >= 0x3041 && c <= 0x3096) // Hiragana.
    || (c >= 0x3099 && c <= 0x309c) // Combining marks.
    || (c >= 0x309d && c <= 0x309e) // Iterating marks.
    || (c >= 0x30a1 && c <= 0x30f6) // Katakana.
    || c == 0x30fc // Prolonged sound mark.
    || (c >= 0x30fd && c <= 0x30fe) // Iterating marks.
}

fn is_hiragana(ch: char) -> bool {
    let c = ch as u32;

    (c >= 0x3041 && c <= 0x3096) // Hiragana.
    || (c >= 0x3099 && c <= 0x309c) // Combining marks.
    || (c >= 0x309d && c <= 0x309e) // Iterating marks.
    || c == 0x30fc // Prolonged sound mark.
    || (c >= 0x30fd && c <= 0x30fe) // Iterating marks.
}

/// Removes all non-kana text from a `&str`, and returns
/// a `String` of the result.
fn strip_non_kana(text: &str) -> String {
    let mut new_text = String::new();
    for ch in text.chars() {
        if is_kana(ch) {
            new_text.push(ch);
        }
    }
    new_text
}

fn hiragana_to_katakana(text: &str) -> String {
    let mut new_text = String::new();
    for ch in text.chars() {
        let c = ch as u32;
        new_text.push(
            if (c >= 0x3041 && c <= 0x3096) || (c >= 0x309d && c <= 0x309e) {
                char::try_from(c + KANA_DIFF).unwrap_or(ch)
            } else {
                ch
            },
        );
    }
    new_text
}

fn katakana_to_hiragana(text: &str) -> String {
    let mut new_text = String::new();
    for ch in text.chars() {
        let c = ch as u32;
        new_text.push(
            if (c >= 0x30a1 && c <= 0x30f6) || (c >= 0x30fd && c <= 0x30fe) {
                char::try_from(c - KANA_DIFF).unwrap_or(ch)
            } else {
                ch
            },
        );
    }
    new_text
}

fn is_all_kana(text: &str) -> bool {
    let mut all_kana = true;
    for ch in text.chars() {
        all_kana &= is_kana(ch);
    }
    all_kana
}

fn is_all_hiragana(text: &str) -> bool {
    let mut all_hiragana = true;
    for ch in text.chars() {
        all_hiragana &= is_hiragana(ch);
    }
    all_hiragana
}
