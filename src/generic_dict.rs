//! Shared types and functions for use in generating all output dictionary
//! formats.

use std::collections::HashMap;

use crate::jmdict;
use crate::yomichan;
use crate::{hiragana_to_katakana, is_all_kana, katakana_to_hiragana};

type TermTable = HashMap<(String, String), Vec<yomichan::TermEntry>>;
type KanjiTable = HashMap<String, Vec<yomichan::KanjiEntry>>;

#[derive(Clone, Debug)]
pub struct Entry {
    // The integer here is a very rough priority ranking indicating
    // the commonness of the word, specifically in that form.  A
    // lower numerical value indicates a more common word.
    pub keys: Vec<(String, u32)>,
    pub definition: String,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LangMode {
    English,    // Standard English terms.
    EnglishAlt, // Alternative English terms, e.g. "self-move" instead of "intransitive".
    Japanese,   // Japanese terms.
}

impl LangMode {
    pub fn idx(&self) -> usize {
        use LangMode::*;
        match *self {
            English => 0,
            EnglishAlt => 1,
            Japanese => 2,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct EntrySettings {
    pub lang_mode: LangMode,
    pub use_katakana_pronunciation: bool,

    /// Whether to include word conjugations in the list of keys to look up
    /// words with.
    pub generate_inflection_keys: bool,
}

pub fn generate_entries(
    yomi_term_table: &TermTable,
    yomi_name_table: &TermTable,
    yomi_kanji_table: &KanjiTable,
    jm_table: &HashMap<(String, String), Vec<jmdict::WordEntry>>,
    pa_table: &HashMap<(String, String), Vec<u32>>,
    entry_settings: EntrySettings,
) -> Vec<Entry> {
    let mut entries = Vec::new();

    // Kanji entries.
    for (kanji, items) in yomi_kanji_table.iter() {
        let mut entry_text: String = "<hr/>".into();
        entry_text.push_str(&generate_kanji_entry_text(&items[0]));

        entries.push(Entry {
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
                    entry_settings,
                    &kana,
                    pitch_accent,
                    &jm_entry,
                ));
                entry_text.push_str(&generate_definition_text(yomi_term_entries));

                // Add to the entry list.
                entries.push(Entry {
                    keys: generate_lookup_keys(jm_entry, entry_settings.generate_inflection_keys),
                    definition: entry_text,
                });
            }
        }
    }

    // Name entries.
    for ((writing, _reading), items) in yomi_name_table.iter() {
        for item in items.iter() {
            let mut entry_text: String = "<hr/>".into();
            entry_text.push_str(&generate_name_entry_text(entry_settings, item));
            entries.push(Entry {
                keys: vec![(writing.clone(), std::u32::MAX)], // Always sort names last.
                definition: entry_text,
            });
        }
    }

    entries.sort_by_key(|a| a.keys[0].0.len());

    entries
}

lazy_static! {
    /// The key is the term, the index of the slice is the mode/language.
    ///
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
    entry_settings: EntrySettings,
    kana: &str,
    pitch_accent: Option<&Vec<u32>>,
    jm_entry: &jmdict::WordEntry,
) -> String {
    let mut text = format!(
        "{}",
        if entry_settings.use_katakana_pronunciation {
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
        jmdict::PartOfSpeech::Verb => {
            use jmdict::ConjugationClass::*;
            let conj_type_text = match jm_entry.conj {
                IchidanVerb => HEADER_TERMS[", ichidan"][entry_settings.lang_mode.idx()],

                GodanVerbU
                | GodanVerbTsu
                | GodanVerbRu
                | GodanVerbKu
                | GodanVerbGu
                | GodanVerbNu
                | GodanVerbBu
                | GodanVerbMu
                | GodanVerbSu => HEADER_TERMS[", godan"][entry_settings.lang_mode.idx()],

                SuruVerb
                | SuruVerbSC
                | KuruVerb
                | IkuVerb
                | KureruVerb
                | AruVerb
                | SharuVerb
                | GodanVerbHu // Doesn't exist in modern Japanese, so we're calling it irregular.
                | IrregularVerb => HEADER_TERMS[", irregular"][entry_settings.lang_mode.idx()],

                _ => "",
            };

            let transitive = jm_entry.tags.contains("pos:vt");
            let intransitive = jm_entry.tags.contains("pos:vi");
            let transitive_text = match (transitive, intransitive) {
                (true, false) => HEADER_TERMS[", transitive"][entry_settings.lang_mode.idx()],
                (false, true) => HEADER_TERMS[", intransitive"][entry_settings.lang_mode.idx()],
                _ => "",
            };

            text.push_str(&format!(
                "{}{}{}{}{}",
                WORD_TYPE_START,
                HEADER_TERMS["verb"][entry_settings.lang_mode.idx()],
                transitive_text,
                conj_type_text,
                WORD_TYPE_END
            ));
        }

        jmdict::PartOfSpeech::Adjective => {
            use jmdict::ConjugationClass::*;
            let adjective_type_text = match jm_entry.conj {
                IAdjective | IrregularIAdjective => {
                    HEADER_TERMS["i-adjective"][entry_settings.lang_mode.idx()]
                }
                _ => HEADER_TERMS["adjective"][entry_settings.lang_mode.idx()],
            };

            let irregular_text = match jm_entry.conj {
                IrregularIAdjective => HEADER_TERMS[", irregular"][entry_settings.lang_mode.idx()],
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

/// Generates the look-up keys for a JMDict word entry.
///
/// If `generate_inflections == true`, then basic conjugations of the word are
/// also added to the key list.
fn generate_lookup_keys(
    jm_entry: &jmdict::WordEntry,
    generate_inflections: bool,
) -> Vec<(String, u32)> {
    use jmdict::ConjugationClass::*;

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

    for word in forms.iter() {
        if !generate_inflections {
            end_replace_push(word, "", &[]);
            continue;
        }

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

fn generate_name_entry_text(entry_settings: EntrySettings, entry: &yomichan::TermEntry) -> String {
    let mut text = String::new();

    if !entry.reading.trim().is_empty() {
        text.push_str(&if entry_settings.use_katakana_pronunciation {
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
    text.push_str(HEADER_TERMS["name"][entry_settings.lang_mode.idx()]);
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
