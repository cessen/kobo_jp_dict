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

use jmdict::{ConjugationClass, Morph, PartOfSpeech};

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
            let reading = strip_non_kana(&hiragana_to_katakana(&morph.readings[0].trim()));
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
            let mut entry_text: String = "<hr/>".into();

            entry_text.push_str(&generate_header_text(
                &kana,
                pa_table.get(&(kanji.clone(), kana.clone())).map(|pa| *pa),
                &morph,
            ));

            // Note: we only include the Japanese definition text if
            // we actually have a kanji writing for the word.  Matching
            // on kana would introduce too many false positive matches.
            if !kanji.is_empty() && !is_all_kana(kanji) {
                entry_text.push_str(&generate_definition_text(
                    &morph,
                    kobo_table
                        .get(&(kanji.clone(), kana.clone()))
                        .map(|a| a.as_slice())
                        .unwrap_or(&[]),
                ));
            } else {
                entry_text.push_str(&generate_definition_text(&morph, &[]));
            }

            entries.push(kobo::Entry {
                keys: generate_lookup_keys(morph),
                definition: entry_text,
            });
        }
    }
    entries.sort_by_key(|a| a.keys[0].len());

    //----------------------------------------------------------------
    // Write the new dictionary file.
    println!("Writing dictionary to disk...");
    kobo::write_dictionary(&entries, std::path::Path::new(output_filename))?;

    return Ok(());
}

/// Generate header text from the given entry information.
fn generate_header_text(kana: &str, pitch_accent: Option<u32>, morph: &Morph) -> String {
    let mut text = format!("{} ", hiragana_to_katakana(&kana),);

    if let Some(p) = pitch_accent {
        text.push_str(&format!("[{}]", p));
    }

    text.push_str(" &nbsp;&nbsp;&mdash; 【");
    let mut first = true;
    if morph.usually_kana || morph.writings.is_empty() {
        text.push_str(&morph.readings[0]);
        first = false;
    }
    for w in morph.writings.iter() {
        if !first {
            text.push_str("／");
        }
        text.push_str(&w);
        first = false;
    }
    text.push_str("】");

    const WORD_TYPE_START: &'static str =
        "<span style=\"font-size: 0.8em; font-style: italic; margin-left: 1.5em;\">";
    const WORD_TYPE_END: &'static str = "</span>";
    match morph.pos {
        PartOfSpeech::Verb => {
            if morph.conj == ConjugationClass::IchidanVerb {
                text.push_str(&format!(
                    "{}{}{}",
                    WORD_TYPE_START, "verb, ichidan", WORD_TYPE_END
                ));
            } else {
                text.push_str(&format!("{}{}{}", WORD_TYPE_START, "verb", WORD_TYPE_END));
            }
        }

        PartOfSpeech::Adjective => {
            text.push_str(&format!(
                "{}{}{}",
                WORD_TYPE_START, "i-adjective", WORD_TYPE_END
            ));
        }

        PartOfSpeech::Expression => {
            text.push_str(&format!(
                "{}{}{}",
                WORD_TYPE_START, "expression", WORD_TYPE_END
            ));
        }

        _ => {}
    }

    text
}

/// Generate English definition text from the given morph.
fn generate_definition_text(morph: &Morph, kobo_entries: &[kobo_ja::Entry]) -> String {
    let mut text = String::new();

    text.push_str("<p style=\"margin-top: 0.7em; margin-bottom: 0.7em;\">");
    for (i, def) in morph.definitions.iter().enumerate() {
        text.push_str(&format!("<b>{}.</b> {}<br/>", i + 1, def));
    }
    text.push_str("</p>");

    for kobo_entry in kobo_entries.iter().take(1) {
        text.push_str(&kobo_entry.definition);
    }

    text
}

/// Generates the look-up keys for a morph, including basic conjugations.
fn generate_lookup_keys(morph: &Morph) -> Vec<String> {
    let mut keys = Vec::new();

    let mut end_replace_push = |word: &str, trail: &str, endings: &[&str]| {
        // We include the katakana version for all-hiragana
        // words as well because for some reason that's how Kobo
        // looks up hiragana words.
        if is_all_kana(word) {
            keys.push(hiragana_to_katakana(word));
        }
        keys.push(word.into());

        if trail.len() > 0 && word.len() >= trail.len() && word.ends_with(trail) {
            let stem = {
                let mut stem: String = word.into();
                stem.truncate(word.len() - trail.len());
                stem
            };

            for end in endings.iter() {
                let variant = format!("{}{}", stem, end);
                if is_all_kana(&variant) {
                    keys.push(hiragana_to_katakana(&variant));
                }
                keys.push(variant);
            }
        }
    };

    let mut forms: Vec<_> = morph.writings.iter().chain(morph.readings.iter()).collect();
    forms.sort();
    forms.dedup();

    use ConjugationClass::*;
    for word in forms.iter() {
        match morph.conj {
            IchidanVerb => {
                end_replace_push(word, "る", &["", "られ", "させ", "ろ", "て", "た"]);
            }

            GodanVerbU => {
                end_replace_push(word, "う", &["わ", "い", "え", "お", "って", "った"]);
            }

            GodanVerbTsu => {
                end_replace_push(word, "つ", &["た", "ち", "て", "と", "って", "った"]);
            }

            GodanVerbRu => {
                end_replace_push(word, "ち", &["ら", "り", "れ", "ろ", "って", "った"]);
            }

            GodanVerbKu => {
                end_replace_push(word, "く", &["か", "き", "け", "こ", "いて", "いた"]);
            }

            GodanVerbGu => {
                end_replace_push(word, "ぐ", &["が", "ぎ", "げ", "ご", "いで", "いだ"]);
            }

            GodanVerbNu => {
                end_replace_push(word, "ぬ", &["な", "に", "ね", "の", "んで", "んだ"]);
            }

            GodanVerbBu => {
                end_replace_push(word, "ぶ", &["ば", "び", "べ", "ぼ", "んで", "んだ"]);
            }

            GodanVerbMu => {
                end_replace_push(word, "む", &["ま", "み", "め", "も", "んで", "んだ"]);
            }

            GodanVerbSu => {
                end_replace_push(word, "す", &["さ", "し", "せ", "そ", "して", "した"]);
            }

            IkuVerb => {
                end_replace_push(word, "く", &["か", "き", "け", "こ", "って", "った"]);
            }

            KuruVerb => {
                end_replace_push(
                    word,
                    "くる",
                    &[
                        "こない",
                        "こなかった",
                        "こなくて",
                        "きて",
                        "きた",
                        "こられ",
                        "こさせ",
                        "こい",
                        "きます",
                        "きません",
                        "きました",
                    ],
                );
                end_replace_push(
                    word,
                    "来る",
                    &[
                        "来ない",
                        "来なかった",
                        "来なくて",
                        "来て",
                        "来た",
                        "来られ",
                        "来させ",
                        "来い",
                        "来ます",
                        "来ません",
                        "来ました",
                    ],
                );
            }

            SuruVerb => {
                end_replace_push(
                    word,
                    "する",
                    &[
                        "しな",
                        "しろ",
                        "させ",
                        "され",
                        "でき",
                        "した",
                        "して",
                        "します",
                        "しません",
                    ],
                );
            }

            IAdjective => {
                end_replace_push(word, "い", &["く", "け", "かった", "かって"]);
            }

            _ => {
                end_replace_push(word, "", &[]);
            }
        };
    }

    keys.sort();
    keys.dedup();
    keys
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
        all_kana &= is_kana(ch);
    }
    all_kana
}
