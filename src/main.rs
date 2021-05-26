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

use jmdict::{ConjugationClass, PartOfSpeech, WordEntry};

fn main() -> io::Result<()> {
    let matches = clap::App::new("Kobo Japanese Dictionary Builder")
        .version(clap::crate_version!())
        .arg(
            clap::Arg::with_name("OUTPUT")
                .help("The output filepath to write the new dictionary to")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::with_name("jmdict")
                .short("j")
                .long("jmdict")
                .help("Path to the JMDict XML file.  This is used as the main source dictionary, and is required")
                .required(true)
                .value_name("PATH")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("pitch_accent")
                .short("p")
                .long("pitch_accent")
                .help("Path to the pitch accent file if available.  Will add pitch accent information to matching entries")
                .value_name("PATH")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("kobo_ja_dict")
                .short("k")
                .long("kobo_ja_dict")
                .help("Path to the Kobo Japanese-Japanese dictionary file if available.  Will add native Japanese definitions to matching entries")
                .value_name("PATH")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("katakana_pronunciation")
                .long("katakana")
                .help("Use katakana instead of hiragana for word pronunciation"),
        )
        .arg(
            clap::Arg::with_name("use_move_terms")
                .short("m")
                .long("use_move_terms")
                .help("Use the terms \"other-move\" and \"self-move\" instead of \"transitive\" and \"intransitive\".  The former is more accurate to how Japanese works, but the latter are more commonly known and used"),
        )
        .get_matches();

    // Output zip archive path.
    let output_filename = matches.value_of("OUTPUT").unwrap();

    //----------------------------------------------------------------
    // Read in all the files.

    // Open and parse the JMDict file.
    let mut jm_table: HashMap<(String, String), Vec<WordEntry>> = HashMap::new(); // (Kanji, Kana)
    if let Some(path) = matches.value_of("jmdict") {
        let parser = jmdict::Parser::from_reader(BufReader::new(File::open(path)?));

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
        println!("JMDict entries: {}", jm_table.len());
    }

    // Open and parse the pitch accent file.
    let mut pa_table: HashMap<(String, String), Vec<u32>> = HashMap::new(); // (Kanji, Kana), Pitch Accent
    if let Some(path) = matches.value_of("pitch_accent") {
        let reader = BufReader::new(File::open(path)?);
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
        println!("Pitch Accent entries: {}", pa_table.len());
    }

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
        println!("Kobo dictionary entries: {}", kobo_table.len());
    }

    //----------------------------------------------------------------
    // Generate the new dictionary entries.
    let mut entries = Vec::new();
    for ((kanji, kana), item) in jm_table.iter() {
        for jm_entry in item.iter() {
            let mut entry_text: String = "<hr/>".into();

            // Find matching entries in other source dictionaries.
            let pitch_accent = pa_table.get(&(kanji.clone(), kana.clone()));
            let kobo_jp_entries = kobo_table
                .get(&(kanji.clone(), kana.clone()))
                .map(|a| a.as_slice())
                .unwrap_or(&[]);

            // Add header and definition to the entry text.
            entry_text.push_str(&generate_header_text(
                matches.is_present("katakana_pronunciation"),
                matches.is_present("use_move_terms"),
                &kana,
                pitch_accent,
                &jm_entry,
            ));
            entry_text.push_str(&generate_definition_text(&jm_entry, kobo_jp_entries));

            // Add to the entry list.
            entries.push(kobo::Entry {
                keys: generate_lookup_keys(jm_entry),
                definition: entry_text,
            });
        }
    }
    entries.sort_by_key(|a| a.keys[0].0.len());

    //----------------------------------------------------------------
    // Write the new dictionary file.
    println!("Writing dictionary to disk...");
    kobo::write_dictionary(&entries, std::path::Path::new(output_filename))?;

    return Ok(());
}

/// Generate header text from the given entry information.
fn generate_header_text(
    use_katakana: bool,
    use_move_terms: bool,
    kana: &str,
    pitch_accent: Option<&Vec<u32>>,
    jm_entry: &WordEntry,
) -> String {
    let mut text = format!(
        "{}",
        if use_katakana {
            hiragana_to_katakana(&kana)
        } else {
            katakana_to_hiragana(&kana)
        }
    );

    if let Some(accent_list) = pitch_accent {
        if !accent_list.is_empty() {
            text.push_str(" ");
            for a in accent_list.iter() {
                text.push_str(&format!("[{}]", a));
            }
        }
    }

    text.push_str(" &nbsp;&nbsp;&mdash; 【");
    let mut first = true;
    if jm_entry.usually_kana || jm_entry.writings.is_empty() {
        text.push_str(&jm_entry.readings[0]);
        first = false;
    }
    for w in jm_entry.writings.iter() {
        if !first {
            text.push_str("／");
        }
        text.push_str(&w);
        first = false;
    }
    text.push_str("】");

    const WORD_TYPE_START: &'static str =
        " <span style=\"font-size: 0.8em; font-style: italic; margin-left: 0; white-space: nowrap;\">";
    const WORD_TYPE_END: &'static str = "</span>";
    match jm_entry.pos {
        PartOfSpeech::Verb => {
            use ConjugationClass::*;
            let conj_type_text = match jm_entry.conj {
                IchidanVerb => ", ichidan",

                GodanVerbU
                | GodanVerbTsu
                | GodanVerbRu
                | GodanVerbKu
                | GodanVerbGu
                | GodanVerbNu
                | GodanVerbBu
                | GodanVerbMu
                | GodanVerbSu => ", godan",

                SuruVerb
                | SuruVerbSC
                | KuruVerb
                | IkuVerb
                | KureruVerb
                | AruVerb
                | SharuVerb
                | GodanVerbHu // Doesn't exist in modern Japanese, so we're calling it irregular.
                | IrregularVerb => ", irregular",

                _ => "",
            };

            let transitive = jm_entry.tags.contains("pos:vt");
            let intransitive = jm_entry.tags.contains("pos:vi");
            let transitive_text = match (transitive, intransitive) {
                (true, false) => {
                    if use_move_terms {
                        ", other-move"
                    } else {
                        ", transitive"
                    }
                }
                (false, true) => {
                    if use_move_terms {
                        ", self-move"
                    } else {
                        ", intransitive"
                    }
                }
                _ => "",
            };

            text.push_str(&format!(
                "{}verb{}{}{}",
                WORD_TYPE_START, conj_type_text, transitive_text, WORD_TYPE_END
            ));
        }

        PartOfSpeech::Adjective => {
            use ConjugationClass::*;
            let adjective_type_text = match jm_entry.conj {
                IAdjective => "i-adjective",

                IrregularIAdjective => "i-adjective, irregular",

                _ => "adjective",
            };

            text.push_str(&format!(
                "{}{}{}",
                WORD_TYPE_START, adjective_type_text, WORD_TYPE_END
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

/// Generate English definition text from the given JMDict entry.
fn generate_definition_text(jm_entry: &WordEntry, kobo_entries: &[kobo_ja::Entry]) -> String {
    let mut text = String::new();

    text.push_str("<p style=\"margin-top: 0.7em; margin-bottom: 0.7em;\">");
    for (i, def) in jm_entry.definitions.iter().enumerate() {
        text.push_str(&format!("<b>{}.</b> {}<br/>", i + 1, def));
    }
    text.push_str("</p>");

    for kobo_entry in kobo_entries.iter().take(1) {
        text.push_str(&kobo_entry.definition);
    }

    text
}

/// Generates the look-up keys for a JMDict word entry, including
/// basic conjugations.
fn generate_lookup_keys(jm_entry: &WordEntry) -> Vec<(String, u32)> {
    let mut keys = Vec::new();

    let mut end_replace_push = |word: &str, trail: &str, endings: &[&str]| {
        // If a word is usually written in kana, give the kana form a major
        // priority boost.
        let priority = if is_all_kana(word) && jm_entry.usually_kana {
            jm_entry.priority / 8
        } else {
            jm_entry.priority
        };

        // We include the katakana version for all-hiragana
        // words as well because for some reason that's how Kobo
        // looks up hiragana words.  Leaving this out causes the Kobo
        // to completely fail to find entries for all-hirigana words.
        if is_all_kana(word) {
            keys.push((hiragana_to_katakana(word), priority));
        }
        keys.push((word.into(), priority));

        if trail.len() > 0 && word.len() >= trail.len() && word.ends_with(trail) {
            let stem = {
                let mut stem: String = word.into();
                stem.truncate(word.len() - trail.len());
                stem
            };

            for end in endings.iter() {
                let variant = format!("{}{}", stem, end);
                if is_all_kana(&variant) {
                    keys.push((hiragana_to_katakana(&variant), priority));
                }
                keys.push((variant, priority));
            }
        }
    };

    let mut forms: Vec<_> = jm_entry
        .writings
        .iter()
        .chain(if jm_entry.usually_kana {
            jm_entry.readings.iter()
        } else {
            (&jm_entry.readings[0..1]).iter()
        })
        .collect();
    forms.sort();
    forms.dedup();

    use ConjugationClass::*;
    for word in forms.iter() {
        match jm_entry.conj {
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
                end_replace_push(word, "い", &["", "く", "け", "かった", "かって"]);
            }

            _ => {
                end_replace_push(word, "", &[]);
            }
        };
    }

    keys.sort_by_key(|a| (a.1, a.0.len(), a.0.clone()));
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
