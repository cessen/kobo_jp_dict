use std::io::BufRead;

use quick_xml::events::Event;

/// A parser for the JMDict xml format, that yields a `Morph`
/// struct for each entry in the dictionary.
pub struct Parser<R: BufRead> {
    xml_parser: quick_xml::Reader<R>,
    buf: Vec<u8>,
    cur_morph: Morph,
    cur_elem: Elem,
}

impl<R: BufRead> Parser<R> {
    pub fn from_reader(reader: R) -> Parser<R> {
        Parser {
            xml_parser: quick_xml::Reader::from_reader(reader),
            buf: Vec::new(),
            cur_morph: Morph::new(),
            cur_elem: Elem::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Morph {
    pub writings: Vec<String>,
    pub readings: Vec<String>,
    pub definitions: Vec<String>,
    pub conj: ConjugationClass,
    pub pos: PartOfSpeech,
    pub usually_kana: bool, // Marks the morph as usually being written in kana alone.
    pub is_onom: bool,      // Marks that a morph is an onomatopoeia.
}

impl Morph {
    pub fn new() -> Morph {
        Morph {
            writings: Vec::new(),
            readings: Vec::new(),
            definitions: Vec::new(),
            conj: ConjugationClass::Other,
            pos: PartOfSpeech::Unknown,
            usually_kana: false,
            is_onom: false,
        }
    }
}

impl<R: BufRead> Iterator for Parser<R> {
    type Item = Morph;

    fn next(&mut self) -> Option<Morph> {
        loop {
            match self.xml_parser.read_event(&mut self.buf) {
                Ok(Event::Start(ref e)) => match e.name() {
                    b"keb" => {
                        self.cur_elem = Elem::Keb;
                    }
                    b"reb" => {
                        self.cur_elem = Elem::Reb;
                    }
                    b"pos" => {
                        self.cur_elem = Elem::Pos;
                    }
                    b"ke_pri" => {
                        self.cur_elem = Elem::WritingPriority;
                    }
                    b"re_pri" => {
                        self.cur_elem = Elem::ReadingPriority;
                    }
                    b"misc" => {
                        self.cur_elem = Elem::Misc;
                    }
                    b"sense" => {
                        self.cur_elem = Elem::Sense;

                        // Start new definition within the morph.
                        if self.cur_morph.definitions.is_empty()
                            || self.cur_morph.definitions.last().unwrap().trim().len() > 0
                        {
                            self.cur_morph.definitions.push("".into());
                        }
                    }
                    b"gloss" => {
                        // If there are no attributes, that means it's
                        // English.  We're ignoring definitions that aren't
                        // written in English.
                        if e.attributes().count() == 0 {
                            self.cur_elem = Elem::Gloss;
                        }
                    }
                    b"name_type" => {
                        self.cur_morph.pos = PartOfSpeech::Noun;
                    }
                    _ => {}
                },
                Ok(Event::End(ref e)) => {
                    self.cur_elem = Elem::None;
                    if e.name() == b"gloss" {
                        // Jump back out into "sense" element.
                        self.cur_elem = Elem::Sense;
                    } else if e.name() == b"sense" {
                        // Remove last two characters, which will just be "; ".
                        self.cur_morph.definitions.last_mut().unwrap().pop();
                        self.cur_morph.definitions.last_mut().unwrap().pop();
                    } else if e.name() == b"entry" {
                        // Clean up the definitions list.
                        if !self.cur_morph.definitions.is_empty()
                            && self.cur_morph.definitions.last().unwrap().trim().is_empty()
                        {
                            self.cur_morph.definitions.pop();
                        }

                        // Reset for next entry, and return the morph.
                        return Some(std::mem::replace(&mut self.cur_morph, Morph::new()));
                    }
                }
                Ok(Event::Text(e)) => {
                    let text = std::str::from_utf8(e.escaped()).unwrap().into();
                    match self.cur_elem {
                        Elem::Gloss => {
                            self.cur_morph
                                .definitions
                                .last_mut()
                                .unwrap()
                                .push_str(&format!("{}; ", text));
                        }
                        Elem::Keb => {
                            self.cur_morph.writings.push(text);
                        }
                        Elem::Reb => {
                            self.cur_morph.readings.push(text);
                        }
                        Elem::Misc => {
                            // Usually written in kana alone.
                            if text == "&uk;" {
                                self.cur_morph.usually_kana = true;
                            }
                            if text == "&on-mim;" {
                                self.cur_morph.is_onom = true;
                            }
                        }
                        Elem::WritingPriority | Elem::ReadingPriority => {}
                        Elem::Pos => {
                            use PartOfSpeech::*;
                            match text.as_str() {
                                // Expression marker.
                                "&exp;" => {
                                    self.cur_morph.pos |= Expression;
                                },

                                // The copula, だ, and words that use it as an ending.
                                "&cop-da;" => {
                                    self.cur_morph.pos |= Copula;
                                    self.cur_morph.conj |= ConjugationClass::Copula;
                                },

                                // i-adjectives.
                                "&adj-i;" => {
                                    self.cur_morph.pos |= Adjective;
                                    self.cur_morph.conj |= ConjugationClass::IAdjective;
                                },

                                // The adjective いい and compounds that end with it.
                                "&adj-ix;" => {
                                    self.cur_morph.pos |= Adjective;
                                    self.cur_morph.conj |= ConjugationClass::IrregularIAdjective;
                                },

                                // Words other than i-adjectives that
                                // (conjugation aside) gramatically behave
                                // similarly to them.  This specifically does
                                // *not* include things like na-adjectives, which
                                // require an additional particle to behave
                                // that way.
                                "&adj-pn;" => { // Pre-noun adjectival.
                                    self.cur_morph.pos |= Adjective;
                                },

                                // Ichidan verbs.
                                "&v1;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::IchidanVerb;
                                },

                                // Godan verbs.
                                "&vn;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::GodanVerbNu;
                                }
                                "&v5u;" | "&v5n;" | "&v4b;" | "&v5b;" | "&v4g;"
                                | "&v5g;" | "&v4h;" | "&v4k;" | "&v5k;" | "&v4m;"
                                | "&v5m;" | "&v4r;" | "&v5r;" | "&v4s;" | "&v5s;"
                                | "&v4t;" | "&v5t;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= match &text[3..4] {
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
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::SuruVerb;
                                },

                                // Verbs ending in する but that don't quite
                                // conjugate like it.
                                "&vs-s;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::SuruVerbSC;
                                },

                                // 来る and verbs that end with it and conjugate
                                // like it.
                                "&vk;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::KuruVerb;
                                },

                                // 行く and verbs that end with it or its variants
                                // (いく and ゆく) and conjugate like it.
                                "&v5k-s;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::IkuVerb;
                                }

                                // Special class of verbs that end with either
                                // さる or しゃる.
                                "&v5aru;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::SharuVerb;
                                },

                                // ある ("to be") and verbs that end with and
                                // conjugate like it.
                                "&v5r-i;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::AruVerb;
                                },

                                // 呉れる / くれる and words the end with it.
                                "&v1-s;" => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::KureruVerb;
                                }

                                // Other irregular verbs.
                                "&vz;" | // ずる verb.
                                "&v5u-s;" // Special class of う verbs.
                                => {
                                    self.cur_morph.pos |= Verb((false, false));
                                    self.cur_morph.conj |= ConjugationClass::IrregularVerb;
                                },

                                // Words that essentially classify as nouns.
                                "&vs;" | // So-called する verb, essentially a nouns.
                                "&adj-na;" | // な adjective, essentially a nouns.
                                "&adj-no;" | // の adjective, essentially a nouns.
                                "&adj-t;" | // たる adjective, essentially a nouns.
                                "&n-adv;" | // Adverbial noun.
                                "&n-pref;" | // Noun used as prefix.
                                "&n-suf;" | // Noun used as suffix.
                                "&n-t;" | // Noun, temporal.
                                "&n;" | // Noun
                                "&pn;" | // Pronoun.
                                "&num;" => {
                                    self.cur_morph.pos |= Noun;
                                }

                                // Adverbs
                                "&adv-to;" |
                                "&adv;" => {
                                    self.cur_morph.pos |= Adverb;
                                }

                                // Particle
                                "&prt;" => {
                                    self.cur_morph.pos |= Particle;
                                }

                                // Conjunction.
                                "&conj;" => {
                                    self.cur_morph.pos |= Conjunction;
                                }

                                // Specifies that a verb is transitive.
                                "&vt;" => {
                                    self.cur_morph.pos |= Verb((true, false));
                                }

                                // Specifies that a verb is intransitive.
                                "&vi;" => {
                                    self.cur_morph.pos |= Verb((false, true));
                                }

                                // Categories that we don't care about or don't know
                                // what to do with right now.
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
    Sense,
    Gloss,
}

//================================================================

// Describes what kind of word the morph is, in terms of its grammatical
// function in a sentence.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum PartOfSpeech {
    Unknown,
    Copula,
    Noun, // Includes na-adjectives, suru-verbs that don't include the
    // suru, etc.  Basically, anything that behaves like a noun unless
    // you put something else with it.
    Particle,
    Conjunction,
    // It might seem weird that we're tracking transitive/intransitive with
    // two bools rather than just one that indicates which of the two the
    // verb is.  But JMDict sometimes specifies neither, and also sometimes
    // different senses of the same word might be different, resulting in both
    // flags being set.  so this lets us capture those things in a little bit
    // more detail so it can be handled appropriately further down the line.
    Verb((bool, bool)), // (transitive, intransitive)
    Adverb,
    Adjective,
    Expression,
}

impl std::ops::BitOr for PartOfSpeech {
    type Output = Self;

    // rhs is the "right-hand side" of the expression `a | b`
    fn bitor(self, rhs: Self) -> Self {
        use PartOfSpeech::*;

        let class_to_priority = |c| match c {
            Copula => 9,
            Particle => 8,
            Conjunction => 7,
            Adjective => 6,
            Adverb => 5,
            Noun => 4,
            Verb(_) => 3,
            Expression => 2,
            Unknown => 0,
        };

        if let (Verb((t1, it1)), Verb((t2, it2))) = (self, rhs) {
            // If they're both verbs, combine the transitivity flags.
            Verb((t1 | t2, it1 | it2))
        } else {
            let self_p = class_to_priority(self);
            let rhs_p = class_to_priority(rhs);

            assert!(
                (self_p != rhs_p) || (self == rhs),
                "Attempt to compose part of speech types with the same priority: {:?} | {:?}",
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
}

impl std::ops::BitOrAssign for PartOfSpeech {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

// Describes the type of conjugation that a given morpheme follows.
//
// The `Other` variant assumes no conjugation, and is used for nouns,
// na-adjectives, etc.  It is also used for archaic words that we don't care
// about and aren't sure how to conjugate.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ConjugationClass {
    // Default.  Assumed not to conjugate.
    Other,

    // だ and words that end with it.
    Copula,

    // Verbs.
    IchidanVerb,
    GodanVerbU,
    GodanVerbTsu,
    GodanVerbRu,
    GodanVerbKu,
    GodanVerbGu,
    GodanVerbNu,
    GodanVerbHu, // Actually yodan, not godan, but follows the same rules.
    GodanVerbBu,
    GodanVerbMu,
    GodanVerbSu,
    SuruVerb,      // する and verbs that end with it and conjugate like it.
    SuruVerbSC,    // Verbs ending in する that don't quite conjugate like it.
    KuruVerb,      // 来る and verbs that end with it and conjugate like it.
    IkuVerb, // 行く or its variants いく and ゆく and words that end with them and conjugate like them.
    KureruVerb, // 呉れる / くれる and verbs that end with it and conjugate like it.
    AruVerb, // ある ("to be") and verbs that end with it and conjugate like it.
    SharuVerb, // Special class of verbs that end with either さる or しゃる.
    IrregularVerb, // Other irregular verbs.  Catch-all for irregular verbs that we don't care about handling for now.

    // Adjectives.
    IAdjective,
    IrregularIAdjective, // いい or compound adjectives that end with いい.
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
