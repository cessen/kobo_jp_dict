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

mod jmdict;
mod kobo;
mod yomichan;

use jmdict::{ConjugationClass, PartOfSpeech, WordEntry};

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
        .get_matches();

    let lang_mode = if matches.is_present("use_japanese_terms") {
        LangMode::Japanese
    } else if matches.is_present("use_move_terms") {
        LangMode::EnglishAlt
    } else {
        LangMode::English
    };

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

    // Open and parse Yomichan dictionaries.
    let mut yomi_term_table: HashMap<(String, String), Vec<yomichan::TermEntry>> = HashMap::new(); // (Kanji, Kana)
    let mut yomi_name_table: HashMap<(String, String), Vec<yomichan::TermEntry>> = HashMap::new(); // (Kanji, Kana)
    let mut yomi_kanji_table: HashMap<String, Vec<yomichan::KanjiEntry>> = HashMap::new(); // Kanji
    if let Some(paths) = matches.values_of("yomichan_dict") {
        for path in paths {
            let mut entry_count = 0usize;

            let (mut word_entries, mut name_entries, mut kanji_entries) =
                yomichan::parse(std::path::Path::new(path)).unwrap();

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
    let mut entries = Vec::new();

    // Kanji entries.
    for (kanji, items) in yomi_kanji_table.iter() {
        let mut entry_text: String = "<hr/>".into();
        entry_text.push_str(&generate_kanji_entry_text(&items[0]));

        entries.push(kobo::Entry {
            keys: vec![(kanji.clone(), 0)],
            definition: entry_text,
        });
    }

    // Term entries.
    for ((kanji, kana), item) in jm_table.iter() {
        for jm_entry in item.iter() {
            // Find matching entries in the source dictionaries.
            let pitch_accent = pa_table.get(&(kanji.clone(), kana.clone()));
            let yomi_term_entries = yomi_term_table
                .get(&(kanji.clone(), kana.clone()))
                .map(|a| a.as_slice())
                .unwrap_or(&[]);

            if pitch_accent.is_some() || !yomi_term_entries.is_empty() {
                let mut entry_text: String = "<hr/>".into();

                // Add header and definition to the entry text.
                entry_text.push_str(&generate_header_text(
                    matches.is_present("katakana_pronunciation"),
                    lang_mode,
                    &kana,
                    pitch_accent,
                    &jm_entry,
                ));
                entry_text.push_str(&generate_definition_text(yomi_term_entries));

                // Add to the entry list.
                entries.push(kobo::Entry {
                    keys: generate_lookup_keys(jm_entry),
                    definition: entry_text,
                });
            }
        }
    }

    // Name entries.
    for ((writing, _reading), items) in yomi_name_table.iter() {
        for item in items.iter() {
            let mut entry_text: String = "<hr/>".into();
            entry_text.push_str(&generate_name_entry_text(
                matches.is_present("katakana_pronunciation"),
                lang_mode,
                item,
            ));
            entries.push(kobo::Entry {
                keys: vec![(writing.clone(), std::u32::MAX)], // Always sort names last.
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum LangMode {
    English,    // Standard English terms.
    EnglishAlt, // Alternative English terms, e.g. "self-move" instead of "intransitive".
    Japanese,   // Japanese terms.
}

impl LangMode {
    fn idx(&self) -> usize {
        use LangMode::*;
        match *self {
            English => 0,
            EnglishAlt => 1,
            Japanese => 2,
        }
    }
}

lazy_static! {
    /// The key is the term, the index of the slice is the mode/language.
    ///
    /// The mode/language index corresponds to LangMode::idx(), above.
    ///
    /// When an entry is missing in a mode/language, it should be
    /// set to the empty string "".
    static ref HEADER_TERMS: HashMap<&'static str, &'static [&'static str]> = {
        let mut m = HashMap::new();

        m.insert("verb", &["verb", "verb", "動詞"][..]);
        m.insert("i-adjective", &["i-adjective", "i-adjective", "形容詞"][..]);
        m.insert("adjective", &["adjective", "adjective", "形容"][..]);
        m.insert("name", &["name", "name", "名"][..]);
        m.insert(
            ", transitive",
            &[", transitive", ", other-move", "、他動"][..],
        );
        m.insert(
            ", intransitive",
            &[", intransitive", ", self-move", "、自動"][..],
        );
        m.insert(", irregular", &[", irregular", ", irregular", ""][..]);
        m.insert(", ichidan", &[", ichidan", ", ichidan", "、一段"][..]);
        m.insert(", godan", &[", godan", ", godan", "、五段"][..]);

        m
    };
}

/// Generate header text from the given entry information.
fn generate_header_text(
    use_katakana: bool,
    lang_mode: LangMode,
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
                IchidanVerb => HEADER_TERMS[", ichidan"][lang_mode.idx()],

                GodanVerbU
                | GodanVerbTsu
                | GodanVerbRu
                | GodanVerbKu
                | GodanVerbGu
                | GodanVerbNu
                | GodanVerbBu
                | GodanVerbMu
                | GodanVerbSu => HEADER_TERMS[", godan"][lang_mode.idx()],

                SuruVerb
                | SuruVerbSC
                | KuruVerb
                | IkuVerb
                | KureruVerb
                | AruVerb
                | SharuVerb
                | GodanVerbHu // Doesn't exist in modern Japanese, so we're calling it irregular.
                | IrregularVerb => HEADER_TERMS[", irregular"][lang_mode.idx()],

                _ => "",
            };

            let transitive = jm_entry.tags.contains("pos:vt");
            let intransitive = jm_entry.tags.contains("pos:vi");
            let transitive_text = match (transitive, intransitive) {
                (true, false) => HEADER_TERMS[", transitive"][lang_mode.idx()],
                (false, true) => HEADER_TERMS[", intransitive"][lang_mode.idx()],
                _ => "",
            };

            text.push_str(&format!(
                "{}{}{}{}{}",
                WORD_TYPE_START,
                HEADER_TERMS["verb"][lang_mode.idx()],
                transitive_text,
                conj_type_text,
                WORD_TYPE_END
            ));
        }

        PartOfSpeech::Adjective => {
            use ConjugationClass::*;
            let adjective_type_text = match jm_entry.conj {
                IAdjective | IrregularIAdjective => HEADER_TERMS["i-adjective"][lang_mode.idx()],
                _ => HEADER_TERMS["adjective"][lang_mode.idx()],
            };

            let irregular_text = match jm_entry.conj {
                IrregularIAdjective => HEADER_TERMS[", irregular"][lang_mode.idx()],
                _ => "",
            };

            text.push_str(&format!(
                "{}{}{}{}",
                WORD_TYPE_START, adjective_type_text, irregular_text, WORD_TYPE_END
            ));
        }

        _ => {}
    }

    text
}

/// Generate English definition text from the given JMDict entry.
fn generate_definition_text(yomi_entries: &[yomichan::TermEntry]) -> String {
    let mut text = String::new();

    text.push_str("<div style=\"margin-top: 0.7em\">");
    for entry in yomi_entries.iter() {
        text.push_str("<p>");
        if yomi_entries.len() > 1 {
            text.push_str(&format!("{}:<br/>", entry.dict_name));
        }
        text.push_str(&yomichan::definition_to_html(
            &entry.definitions,
            entry.definitions.depth(),
            true,
        ));
        text.push_str("</p>");
    }
    text.push_str("</div>");

    text
}

/// Generates the look-up keys for a JMDict word entry, including
/// basic conjugations.
fn generate_lookup_keys(jm_entry: &WordEntry) -> Vec<(String, u32)> {
    let jm_priority = jm_entry.priority + 256; // Ensure we never reach zero, since that's reserved for Kanji entries.

    // Give verbs and i-adjectives a priority boost, so they show up
    // earlier in search results.
    let priority_boost = match jm_entry.conj {
        IchidanVerb | GodanVerbU | GodanVerbTsu | GodanVerbRu | GodanVerbKu | GodanVerbGu
        | GodanVerbNu | GodanVerbBu | GodanVerbMu | GodanVerbSu | IkuVerb | KuruVerb | SuruVerb => {
            4
        }
        IAdjective => 2,
        _ => 1,
    };

    let mut keys = Vec::new();

    let mut end_replace_push = |word: &str, trail: &str, endings: &[&str]| {
        // If a word is usually written in kana, give the kana form a major
        // priority boost.
        let priority = if is_all_kana(word) && jm_entry.usually_kana {
            jm_priority / 8
        } else {
            jm_priority
        } / priority_boost;

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
            // We include the ～あない ending even though it should be covered by ～あ because
            // there are some entries for exactly ～あない, and they prevent the verb entries
            // from showing up.
            IchidanVerb => {
                end_replace_push(word, "る", &["", "ない", "られ", "させ", "ろ", "て", "た"]);
            }

            GodanVerbU => {
                end_replace_push(
                    word,
                    "う",
                    &["わない", "わ", "い", "え", "お", "って", "った"],
                );
            }

            GodanVerbTsu => {
                end_replace_push(
                    word,
                    "つ",
                    &["たない", "た", "ち", "て", "と", "って", "った"],
                );
            }

            GodanVerbRu => {
                end_replace_push(
                    word,
                    "る",
                    &["らない", "ら", "り", "れ", "ろ", "って", "った"],
                );
            }

            GodanVerbKu => {
                end_replace_push(
                    word,
                    "く",
                    &["かない", "か", "き", "け", "こ", "いて", "いた"],
                );
            }

            GodanVerbGu => {
                end_replace_push(
                    word,
                    "ぐ",
                    &["がない", "が", "ぎ", "げ", "ご", "いで", "いだ"],
                );
            }

            GodanVerbNu => {
                end_replace_push(
                    word,
                    "ぬ",
                    &["なない", "な", "に", "ね", "の", "んで", "んだ"],
                );
            }

            GodanVerbBu => {
                end_replace_push(
                    word,
                    "ぶ",
                    &["ばない", "ば", "び", "べ", "ぼ", "んで", "んだ"],
                );
            }

            GodanVerbMu => {
                end_replace_push(
                    word,
                    "む",
                    &["まない", "ま", "み", "め", "も", "んで", "んだ"],
                );
            }

            GodanVerbSu => {
                end_replace_push(
                    word,
                    "す",
                    &["さない", "さ", "し", "せ", "そ", "して", "した"],
                );
            }

            IkuVerb => {
                end_replace_push(
                    word,
                    "く",
                    &["かない", "か", "き", "け", "こ", "って", "った"],
                );
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
                        "しない",
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

fn generate_name_entry_text(
    use_katakana: bool,
    lang_mode: LangMode,
    entry: &yomichan::TermEntry,
) -> String {
    let mut text = String::new();

    if !entry.reading.trim().is_empty() {
        text.push_str(&if use_katakana {
            hiragana_to_katakana(&entry.reading)
        } else {
            katakana_to_hiragana(&entry.reading)
        });
        text.push_str(" &nbsp;&nbsp;&mdash; ");
    }

    text.push_str("【");
    text.push_str(&entry.writing);
    text.push_str("】");

    const WORD_TYPE_START: &'static str =
        " <span style=\"font-size: 0.8em; font-style: italic; margin-left: 0; white-space: nowrap;\">";
    const WORD_TYPE_END: &'static str = "</span>";
    text.push_str(WORD_TYPE_START);
    text.push_str(HEADER_TERMS["name"][lang_mode.idx()]);
    if !entry.tags.is_empty() {
        text.push_str(": ");
        for tag in entry.tags.iter() {
            text.push_str(tag);
            text.push_str(", ");
        }
        text.pop();
        text.pop();
    }
    text.push_str(WORD_TYPE_END);

    if !entry.definitions.is_empty() {
        text.push_str(&yomichan::definition_to_html(
            &entry.definitions,
            entry.definitions.depth(),
            false,
        ));
    }

    text
}

fn generate_kanji_entry_text(entry: &yomichan::KanjiEntry) -> String {
    let mut text = String::new();

    text.push_str("<p style=\"margin-left: 2.5em; margin-bottom: 1.0em; text-indent: -2.5em;\"><span style=\"font-size: 2.0em;\">");
    text.push_str(&entry.kanji);
    if !entry.meanings.is_empty() {
        text.push_str("</span>　");
        for meaning in entry.meanings.iter() {
            text.push_str(meaning);
            text.push_str(", ");
        }
        text.pop();
        text.pop();
    }
    text.push_str("</p>");

    if !entry.onyomi.is_empty() {
        text.push_str("<p style=\"margin-left: 2.5em; text-indent: -2.5em;\">音:　");
        for onyomi in entry.onyomi.iter() {
            text.push_str(onyomi);
            text.push_str("／");
        }
        text.pop();
        text.push_str("</p>");
    }

    if !entry.kunyomi.is_empty() {
        text.push_str("<p style=\"margin-left: 2.5em; text-indent: -2.5em;\">訓:　");
        for kunyomi in entry.kunyomi.iter() {
            text.push_str(kunyomi);
            text.push_str("／");
        }
        text.pop();
        text.push_str("</p>");
    }

    text
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
