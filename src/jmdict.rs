//! This file contains code for parsing JMDict XML files.
//!
//! The `Parser` type takes a buffered reader, and acts as an iterator
//! that yields a `WordEntry` for each entry in the dictionary, parsing
//! the input as it goes.

use std::collections::HashSet;
use std::io::BufRead;

use quick_xml::events::Event;

/// A parser for the JMDict xml format.
pub struct Parser<R: BufRead> {
    xml_parser: quick_xml::Reader<R>,
    buf: Vec<u8>,
    cur_entry: WordEntry,
    kanji_priorities: Vec<String>,
    kana_priorities: Vec<String>,
    cur_xml_elem: Elem,
}

impl<R: BufRead> Parser<R> {
    pub fn from_reader(reader: R) -> Parser<R> {
        Parser {
            xml_parser: quick_xml::Reader::from_reader(reader),
            buf: Vec::new(),
            cur_entry: WordEntry::new(),
            kanji_priorities: Vec::new(),
            kana_priorities: Vec::new(),
            cur_xml_elem: Elem::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WordEntry {
    pub writings: Vec<String>, // Kanji-based writings of the word.
    pub readings: Vec<String>, // Furigana and kana-based writings of the word.
    pub definitions: Vec<String>,
    pub conj: ConjugationClass,
    pub pos: PartOfSpeech,
    pub usually_kana: bool, // When true, indicates that the word is usually written in kana alone.

    // A very rough priority ranking indicating the commonness of the word.
    // A lower numerical value indicates a more common word.
    pub priority: u32,

    // Set of tags found, in the format "parent_element:entity".
    // For example, if "<pos>&conj;</pos>" is found in the xml, then there
    // will be an entry "pos:conj" in this set.
    // This can give more detailed information about the word than the
    // filtered and processed struct fields above, when needed.
    // See the JMDict XML file for details about possible tags.
    pub tags: HashSet<String>,
}

impl WordEntry {
    pub fn new() -> WordEntry {
        WordEntry {
            writings: Vec::new(),
            readings: Vec::new(),
            definitions: Vec::new(),
            conj: ConjugationClass::Other,
            pos: PartOfSpeech::Unknown,
            usually_kana: false,
            priority: 100000,
            tags: HashSet::new(),
        }
    }
}

/// Indicates the conjugation rules that a word follows.
///
/// The `Other` variant indicates a word that either doesn't conjugate (such
/// as nouns, na-adjectives, etc.), or a word whose conjugations rules are
/// unclear due to being e.g. archaic.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ConjugationClass {
    // Default.  Assumed not to conjugate.
    Other,

    // だ and words that end with it.
    Copula,

    // Regular verbs.
    IchidanVerb,
    GodanVerbU,
    GodanVerbTsu,
    GodanVerbRu,
    GodanVerbKu,
    GodanVerbGu,
    GodanVerbNu,
    GodanVerbHu, // Doesn't exist in modern Japanese, but does in classical Japanese.
    GodanVerbBu,
    GodanVerbMu,
    GodanVerbSu,

    // Irregular verbs.
    SuruVerb,      // する and verbs that end with it and conjugate like it.
    SuruVerbSC,    // Verbs ending in する that don't quite conjugate like it.
    KuruVerb,      // 来る and verbs that end with it and conjugate like it.
    IkuVerb,       // 行く and verbs that end with it and conjugate like it.
    KureruVerb,    // 呉れる / くれる and verbs that end with it and conjugate like it.
    AruVerb,       // ある ("to be") and verbs that end with it and conjugate like it.
    SharuVerb,     // Special class of verbs that end with either さる or しゃる.
    IrregularVerb, // Catch-all for other irregular verbs.

    // Adjectives.
    IAdjective,
    IrregularIAdjective, // いい or compound adjectives that end with いい.
}

/// Indicates a word's grammatical role.
///
/// In reality, the specifics of this go
/// much deeper than what's represented here.  This is just a broad
/// surface-level categorization.  More detailed breakdowns can be accessed
/// in `WordEntry::tags` when needed.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum PartOfSpeech {
    Unknown,
    Copula,
    Noun, // Includes na-adjectives, suru-verbs that don't include the
    // suru, etc.  Basically, anything that behaves like a noun unless
    // you put something else with it.
    Particle,
    Conjunction,
    Verb,
    Adverb,
    Adjective, // i-adjectives only.  Na-adjectives are actually nouns.
    Expression,
}

//================================================================
// Parser implementation.

impl<R: BufRead> Iterator for Parser<R> {
    type Item = WordEntry;

    fn next(&mut self) -> Option<WordEntry> {
        fn add_tag(entry: &mut WordEntry, elem: &str, tag: &str) {
            let tag = tag.trim();
            if tag.starts_with("&") && tag.ends_with(";") {
                entry
                    .tags
                    .insert(format!("{}:{}", elem, (&tag[1..(tag.len() - 1)])));
            }
        }

        loop {
            match self.xml_parser.read_event(&mut self.buf) {
                Ok(Event::Start(ref e)) => match e.name() {
                    b"keb" => {
                        self.cur_xml_elem = Elem::Keb;
                    }
                    b"reb" => {
                        self.cur_xml_elem = Elem::Reb;
                    }
                    b"pos" => {
                        self.cur_xml_elem = Elem::Pos;
                    }
                    b"ke_pri" => {
                        self.cur_xml_elem = Elem::WritingPriority;
                    }
                    b"re_pri" => {
                        self.cur_xml_elem = Elem::ReadingPriority;
                    }
                    b"misc" => {
                        self.cur_xml_elem = Elem::Misc;
                    }
                    b"dial" => {
                        self.cur_xml_elem = Elem::Dialect;
                    }
                    b"field" => {
                        self.cur_xml_elem = Elem::Field;
                    }
                    b"sense" => {
                        self.cur_xml_elem = Elem::Sense;

                        // Start new definition within the entry.
                        if self.cur_entry.definitions.is_empty()
                            || self.cur_entry.definitions.last().unwrap().trim().len() > 0
                        {
                            self.cur_entry.definitions.push("".into());
                        }
                    }
                    b"gloss" => {
                        // If there are no attributes, that means it's
                        // English.  We're ignoring definitions that aren't
                        // written in English.
                        if e.attributes().count() == 0 {
                            self.cur_xml_elem = Elem::Gloss;
                        }
                    }
                    b"name_type" => {
                        self.cur_entry.pos = PartOfSpeech::Noun;
                    }
                    _ => {}
                },
                Ok(Event::End(ref e)) => {
                    self.cur_xml_elem = Elem::None;
                    if e.name() == b"gloss" {
                        // Jump back out into "sense" element.
                        self.cur_xml_elem = Elem::Sense;
                    } else if e.name() == b"sense" {
                        // Remove last two characters, which will just be "; ".
                        self.cur_entry.definitions.last_mut().unwrap().pop();
                        self.cur_entry.definitions.last_mut().unwrap().pop();
                    } else if e.name() == b"entry" {
                        // Clean up the definitions list.
                        if !self.cur_entry.definitions.is_empty()
                            && self.cur_entry.definitions.last().unwrap().trim().is_empty()
                        {
                            self.cur_entry.definitions.pop();
                        }

                        // If there are no kanji writings, make sure it's
                        // marked as "usually kana", because JMDict forgets
                        // this sometimes (or possibly just assumes it's
                        // implicit).
                        if self.cur_entry.writings.is_empty() {
                            self.cur_entry.usually_kana = true;
                        }

                        // Calculate word priority.
                        let priorities = if self.cur_entry.usually_kana {
                            &self.kana_priorities
                        } else {
                            &self.kanji_priorities
                        };
                        for p_text in priorities.iter() {
                            let p = if p_text.starts_with("nf") {
                                (&p_text[2..]).parse::<u32>().unwrap().saturating_sub(1) * 500
                            } else {
                                match p_text.as_str() {
                                    "news1" | "ichi1" | "gai1" => 6000,
                                    "news2" | "ichi2" | "gai2" => 18000,
                                    _ => 24000,
                                }
                            };
                            self.cur_entry.priority = self.cur_entry.priority.min(p);
                        }

                        // Reset for next entry, and return the `WordEntry`.
                        self.kanji_priorities.clear();
                        self.kana_priorities.clear();
                        return Some(std::mem::replace(&mut self.cur_entry, WordEntry::new()));
                    }
                }
                Ok(Event::Text(e)) => {
                    let text = std::str::from_utf8(e.escaped()).unwrap().into();
                    match self.cur_xml_elem {
                        Elem::Gloss => {
                            self.cur_entry
                                .definitions
                                .last_mut()
                                .unwrap()
                                .push_str(&format!("{}; ", text));
                        }
                        Elem::Keb => {
                            self.cur_entry.writings.push(text);
                        }
                        Elem::Reb => {
                            self.cur_entry.readings.push(text);
                        }
                        Elem::Misc => {
                            add_tag(&mut self.cur_entry, "misc", &text);

                            // Usually written in kana alone.
                            if text == "&uk;" {
                                self.cur_entry.usually_kana = true;
                            }
                        }
                        Elem::Dialect => {
                            add_tag(&mut self.cur_entry, "dial", &text);
                        }
                        Elem::Field => {
                            add_tag(&mut self.cur_entry, "field", &text);
                        }
                        Elem::WritingPriority => {
                            self.kanji_priorities.push(text.trim().into());
                        }
                        Elem::ReadingPriority => {
                            self.kana_priorities.push(text.trim().into());
                        }
                        Elem::Pos => {
                            add_tag(&mut self.cur_entry, "pos", &text);

                            use PartOfSpeech::*;
                            match text.as_str() {
                                // Expression marker.
                                "&exp;" => {
                                    self.cur_entry.pos |= Expression;
                                },

                                // The copula, だ, and words that use it as an ending.
                                "&cop-da;" => {
                                    self.cur_entry.pos |= Copula;
                                    self.cur_entry.conj |= ConjugationClass::Copula;
                                },

                                // i-adjectives.
                                "&adj-i;" => {
                                    self.cur_entry.pos |= Adjective;
                                    self.cur_entry.conj |= ConjugationClass::IAdjective;
                                },

                                // The adjective いい and compounds that end with it.
                                "&adj-ix;" => {
                                    self.cur_entry.pos |= Adjective;
                                    self.cur_entry.conj |= ConjugationClass::IrregularIAdjective;
                                },

                                // Words other than i-adjectives that
                                // (conjugation aside) gramatically behave
                                // similarly to them.  This specifically does
                                // *not* include things like na-adjectives, which
                                // require an additional particle to behave
                                // that way.
                                "&adj-pn;" => { // Pre-noun adjectival.
                                    self.cur_entry.pos |= Adjective;
                                },

                                // Ichidan verbs.
                                "&v1;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::IchidanVerb;
                                },

                                // Godan verbs.
                                "&vn;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::GodanVerbNu;
                                }
                                "&v5u;" | "&v5n;" | "&v4b;" | "&v5b;" | "&v4g;"
                                | "&v5g;" | "&v4h;" | "&v4k;" | "&v5k;" | "&v4m;"
                                | "&v5m;" | "&v4r;" | "&v5r;" | "&v4s;" | "&v5s;"
                                | "&v4t;" | "&v5t;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= match &text[3..4] {
                                        "u" => ConjugationClass::GodanVerbU,
                                        "t" => ConjugationClass::GodanVerbTsu,
                                        "r" => ConjugationClass::GodanVerbRu,
                                        "k" => ConjugationClass::GodanVerbKu,
                                        "g" => ConjugationClass::GodanVerbGu,
                                        "n" => ConjugationClass::GodanVerbNu,
                                        "h" => ConjugationClass::GodanVerbHu,
                                        "b" => ConjugationClass::GodanVerbBu,
                                        "m" => ConjugationClass::GodanVerbMu,
                                        "s" => ConjugationClass::GodanVerbSu,
                                        _ => unreachable!(),
                                    };
                                }

                                // する and verbs that end with it and conjugate
                                // like it.
                                "&vs-i;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::SuruVerb;
                                },

                                // Verbs ending in する but that don't quite
                                // conjugate like it.
                                "&vs-s;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::SuruVerbSC;
                                },

                                // 来る and verbs that end with it and conjugate
                                // like it.
                                "&vk;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::KuruVerb;
                                },

                                // 行く and verbs that end with it or its variants
                                // (いく and ゆく) and conjugate like it.
                                "&v5k-s;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::IkuVerb;
                                }

                                // Special class of verbs that end with either
                                // さる or しゃる.
                                "&v5aru;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::SharuVerb;
                                },

                                // ある ("to be") and verbs that end with and
                                // conjugate like it.
                                "&v5r-i;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::AruVerb;
                                },

                                // 呉れる / くれる and words the end with it.
                                "&v1-s;" => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::KureruVerb;
                                }

                                // Other irregular verbs.
                                "&vz;" | // ずる verb.
                                "&v5u-s;" // Special class of う verbs.
                                => {
                                    self.cur_entry.pos |= Verb;
                                    self.cur_entry.conj |= ConjugationClass::IrregularVerb;
                                },

                                // Words that essentially classify as nouns.
                                "&vs;" | // So-called する verb, grammatically a noun.
                                "&adj-na;" | // な adjective, grammatically a noun.
                                "&adj-no;" | // の adjective, grammatically a noun.
                                "&adj-t;" | // たる adjective, grammatically a noun.
                                "&n-adv;" | // Adverbial noun.
                                "&n-pref;" | // Noun used as prefix.
                                "&n-suf;" | // Noun used as suffix.
                                "&n-t;" | // Noun, temporal.
                                "&n;" | // Noun
                                "&pn;" | // Pronoun.
                                "&num;" => {
                                    self.cur_entry.pos |= Noun;
                                }

                                // Adverbs
                                "&adv-to;" |
                                "&adv;" => {
                                    self.cur_entry.pos |= Adverb;
                                }

                                // Particle
                                "&prt;" => {
                                    self.cur_entry.pos |= Particle;
                                }

                                // Conjunction.
                                "&conj;" => {
                                    self.cur_entry.pos |= Conjunction;
                                }

                                // Categories that we don't care about or don't know
                                // what to do with right now.
                                "&vt;" | // Transitive verb.
                                "&vi;" | // Intransitive verb.
                                "&adj-f;" | // Noun or verb acting prenominally.
                                "&ctr;" | // Counter.
                                "&int;" | // Interjection.
                                "&aux;" | // Auxiliary.
                                "&aux-v;" | // Auxiliary verb.
                                "&aux-adj;" | // Auxiliary adjective.
                                "&pref;" | // Prefix.
                                "&suf;" | // Suffix.
                                "&unc;" | // Unclassified.
                                // Archaic verbs.
                                "&adj-kari;" | // Archaic.
                                "&adj-ku;" | // Archaic.
                                "&adj-nari;" | // Archaic.
                                "&adj-shiku;" | // Archaic.
                                "&vr;" | // Irregular る verb whose plain ending is り. Pretty much all archaic.
                                "&vs-c;" | // Precursors to する, archaic.
                                "&v2a-s;" | // Nidan verb, archaic.
                                "&v2b-k;" | // Nidan verb, archaic.
                                "&v2d-s;" | // Nidan verb, archaic.
                                "&v2g-k;" | // Nidan verb, archaic.
                                "&v2g-s;" | // Nidan verb, archaic.
                                "&v2h-k;" | // Nidan verb, archaic.
                                "&v2h-s;" | // Nidan verb, archaic.
                                "&v2k-k;" | // Nidan verb, archaic.
                                "&v2k-s;" | // Nidan verb, archaic.
                                "&v2m-s;" | // Nidan verb, archaic.
                                "&v2n-s;" | // Nidan verb, archaic.
                                "&v2r-k;" | // Nidan verb, archaic.
                                "&v2r-s;" | // Nidan verb, archaic.
                                "&v2s-s;" | // Nidan verb, archaic.
                                "&v2t-k;" | // Nidan verb, archaic.
                                "&v2t-s;" | // Nidan verb, archaic.
                                "&v2w-s;" | // Nidan verb, archaic.
                                "&v2y-k;" | // Nidan verb, archaic.
                                "&v2y-s;" | // Nidan verb, archaic.
                                "&v2z-s;" // Nidan verb, archaic.
                                => {
                                },

                                // Unknown classification string.
                                _ => {
                                }
                            }
                        }
                        Elem::Sense => {}
                        Elem::None => {}
                    }
                }
                Err(e) => {
                    panic!(
                        "Error at position {}: {:?}",
                        self.xml_parser.buffer_position(),
                        e
                    )
                }
                Ok(Event::Eof) => {
                    return None;
                }
                _ => (),
            }
            self.buf.clear();
        }
    }
}

enum Elem {
    None,
    Keb,
    Reb,
    Pos,
    WritingPriority,
    ReadingPriority,
    Misc,
    Dialect,
    Field,
    Sense,
    Gloss,
}

//================================================================
// Impls for other types in this file.

impl std::ops::BitOr for PartOfSpeech {
    type Output = Self;

    // rhs is the "right-hand side" of the expression `a | b`
    fn bitor(self, rhs: Self) -> Self {
        use PartOfSpeech::*;

        let class_to_priority = |c| match c {
            Copula => 9,
            Particle => 8,
            Conjunction => 7,
            Verb => 6,
            Adjective => 5,
            Adverb => 4,
            Noun => 3,
            Expression => 2,
            Unknown => 0,
        };

        let self_p = class_to_priority(self);
        let rhs_p = class_to_priority(rhs);

        assert!(
            (self_p != rhs_p) || (self == rhs),
            "Attempt to compose different part-of-speech types with the same priority: {:?} | {:?}",
            self,
            rhs
        );

        if self_p >= rhs_p {
            self
        } else {
            rhs
        }
    }
}

impl std::ops::BitOrAssign for PartOfSpeech {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

impl std::ops::BitOr for ConjugationClass {
    type Output = Self;

    // rhs is the "right-hand side" of the expression `a | b`
    fn bitor(self, rhs: Self) -> Self {
        use ConjugationClass::*;

        let class_to_priority = |c| match c {
            Copula => 8,

            SuruVerbSC => 7,

            SuruVerb | KuruVerb | IkuVerb | KureruVerb | AruVerb | SharuVerb => 6,

            IrregularVerb => 5,

            GodanVerbU | GodanVerbTsu | GodanVerbRu | GodanVerbKu | GodanVerbGu | GodanVerbNu
            | GodanVerbHu | GodanVerbBu | GodanVerbMu | GodanVerbSu => 4,

            IchidanVerb => 3,

            IrregularIAdjective => 2,
            IAdjective => 1,

            Other => 0,
        };

        let self_p = class_to_priority(self);
        let rhs_p = class_to_priority(rhs);

        assert!(
            (self_p != rhs_p) || (self == rhs),
            "Attempt to compose conjugation types with the same priority: {:?} | {:?}",
            self,
            rhs
        );

        if self_p > rhs_p {
            self
        } else {
            rhs
        }
    }
}

impl std::ops::BitOrAssign for ConjugationClass {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}
