#![allow(dead_code)]

use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;

mod jmdict;
mod kobo;
mod kobo_ja;

use jmdict::Morph;

fn main() -> io::Result<()> {
    let matches = clap::App::new("Kobo Japanese Dictionary Builder")
        .version(clap::crate_version!())
        .arg(
            clap::Arg::with_name("OUTPUT")
                .help("Sets the output file to create")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::with_name("kobo_ja_dict")
                .short("k")
                .long("kobo_ja_dict")
                .help("Path to the Kobo Japanese-Japanese dictionary file if available")
                .value_name("PATH")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("jmdict")
                .short("j")
                .long("jmdict")
                .help("Path to the JMDict file if available")
                .value_name("PATH")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("pitch_accent")
                .short("p")
                .long("pitch_accent")
                .help("Path to the pitch accent file if available")
                .value_name("PATH")
                .takes_value(true),
        )
        .get_matches();

    // Output zip archive path.
    let output_filename = matches.value_of("OUTPUT").unwrap();

    //----------------------------------------------------------------
    // Read in all the files.

    // Open and parse the JMDict file.
    let mut jm_table: HashMap<(String, String), Vec<Morph>> = HashMap::new(); // (Kanji, Kana)
    if let Some(path) = matches.value_of("jmdict") {
        let parser = jmdict::Parser::from_reader(BufReader::new(File::open(path)?));

        for morph in parser {
            let reading = hiragana_to_katakana(&morph.readings[0]);
            let writing = if morph.writings.len() > 0 {
                morph.writings[0].clone()
            } else {
                reading.clone()
            };

            let e = jm_table.entry((writing, reading)).or_insert(Vec::new());
            e.push(morph);
        }
    }
    println!("JMDict entries: {}", jm_table.len());

    // Open and parse the pitch accent file.
    let mut pa_table: HashMap<(String, String), u32> = HashMap::new(); // (Kanji, Kana), Pitch Accent
    if let Some(path) = matches.value_of("pitch_accent") {
        let reader = BufReader::new(File::open(path)?);
        for line in reader.lines() {
            let line = line.unwrap_or_else(|_| "".into());
            if line.chars().nth(0).unwrap_or('\n').is_digit(10) {
                let parts: Vec<_> = line.split("\t").collect();
                assert_eq!(parts.len(), 7);
                if let Ok(accent) = parts[5].parse::<u32>() {
                    pa_table.insert((parts[1].into(), hiragana_to_katakana(parts[2])), accent);
                }
            }
        }
    }
    println!("JA Accent entries: {}", pa_table.len());

    // Open and parse the Kobo Japanese-Japanese dictionary.
    let mut kobo_table: HashMap<(String, String), Vec<kobo_ja::Entry>> = HashMap::new(); // (DictKey, Kana) -> EntryList
    if let Some(path) = matches.value_of("kobo_ja_dict") {
        let mut entries = kobo_ja::parse(std::path::Path::new(path), true)?;
        for entry in entries.drain(..) {
            let entry_list = kobo_table
                .entry((entry.key.clone(), entry.kana.clone()))
                .or_insert(Vec::new());
            entry_list.push(entry);
        }
    }
    println!("Kobo dictionary entries: {}", kobo_table.len());

    //----------------------------------------------------------------
    // Generate the new dictionary entries.
    let mut entries = Vec::new();
    for ((kanji, kana), item) in jm_table.iter() {
        for morph in item.iter() {
            let mut definition: String = "<hr/>".into();

            let mut writings = morph.writings.clone();
            if morph.usually_kana {
                writings.insert(0, morph.readings[0].clone());
            }
            definition.push_str(&generate_header_text(
                &kana,
                pa_table.get(&(kanji.clone(), kana.clone())).map(|pa| *pa),
                &writings,
            ));
            definition.push_str(&generate_definition_text(&morph));

            entries.push(kobo::Entry {
                keys: generate_lookup_keys(morph),
                definition,
            });
        }
    }

    //----------------------------------------------------------------
    // Write the new dictionary file.
    println!("Writing dictionary to disk...");
    kobo::write_dictionary(&entries, std::path::Path::new(output_filename))?;

    return Ok(());
}

/// Generate header text from the given entry information.
fn generate_header_text(kana: &str, pitch_accent: Option<u32>, writings: &[String]) -> String {
    let mut text = format!("{} ", hiragana_to_katakana(&kana),);

    if let Some(p) = pitch_accent {
        text.push_str(&format!("[{}]", p));
    }

    text.push_str(" &nbsp;&nbsp;&mdash; 【");

    let mut first = true;
    for w in writings.iter() {
        if !first {
            text.push_str("／");
        }
        text.push_str(w);
        first = false;
    }
    text.push_str("】");

    text
}

/// Generate English definition text from the given morph.
fn generate_definition_text(morph: &Morph) -> String {
    let mut text = String::new();

    text.push_str("<p style=\"margin-top: 0.6em; margin-bottom: 0.6em;\">");
    for (i, def) in morph.definitions.iter().enumerate() {
        text.push_str(&format!("<b>{}.</b> {}<br/>", i + 1, def));
    }
    text.push_str("</p>");

    text
}

/// Generates the look-up keys for a morph, including basic conjugations.
fn generate_lookup_keys(morph: &Morph) -> Vec<String> {
    morph
        .writings
        .iter()
        .chain(morph.readings.iter())
        .map(|s| (*s).clone())
        .collect()
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
        all_kana |= is_kana(ch);
    }
    all_kana
}
