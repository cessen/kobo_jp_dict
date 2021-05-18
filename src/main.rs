#![allow(dead_code)]

use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::{BufReader, BufWriter};

use flate2::read::{GzDecoder, GzEncoder};
use quick_xml::{events::Event, Reader};

mod jmdict;
use jmdict::Morph;

fn main() -> io::Result<()> {
    let matches = clap::App::new("Kobo Japanese Dictionary Merger")
        .version(clap::crate_version!())
        .arg(
            clap::Arg::with_name("INPUT")
                .help("Sets the input file to use")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::with_name("OUTPUT")
                .help("Sets the output file to create")
                .required(true)
                .index(2),
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

    // Open the input zip archive.
    let input_filename = matches.value_of("INPUT").unwrap();
    let mut zip_in = zip::ZipArchive::new(BufReader::new(File::open(input_filename)?))?;

    // Open the output zip archive.
    let output_filename = matches.value_of("OUTPUT").unwrap();
    let mut zip_out = zip::ZipWriter::new(BufWriter::new(File::create(output_filename)?));

    // Open and parse the JMDict file.
    let mut jm_table: HashMap<(String, String), Morph> = HashMap::new(); // (Kanji, Kana)
    if let Some(path) = matches.value_of("jmdict") {
        let parser = jmdict::Parser::from_reader(BufReader::new(File::open(path)?));

        for morph in parser {
            let reading = hiragana_to_katakana(&morph.readings[0]);
            if morph.writings.len() > 0
                && !jm_table.contains_key(&(morph.writings[0].clone(), reading.clone()))
            {
                jm_table.insert((morph.writings[0].clone(), reading), morph);
            }
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

    // Loop through all files in the zip file, processing each
    // one appropriately before writing it to the output zip
    // file.
    let mut data = Vec::new();
    let mut html = String::new();
    let mut html_processed = String::new();
    let mut gzhtml = Vec::new();
    for i in 0..zip_in.len() {
        print!("\r{}/{}", i + 1, zip_in.len());
        let mut f = zip_in.by_index(i).unwrap();
        f.read_to_end(&mut data).unwrap();
        let name_raw = f.name_raw();

        // HTML files
        if name_raw.len() >= 5 && &name_raw[(name_raw.len() - 5)..] == &b".html"[..] {
            // Decompress html data.
            let mut ungz = GzDecoder::new(&data[..]);
            html.clear();
            ungz.read_to_string(&mut html).unwrap();

            // Process the html as desired.
            html_processed.clear();
            process_entries(&html, &mut html_processed, &jm_table, &pa_table);

            // Recompress html data.
            let mut gz = GzEncoder::new(html_processed.as_bytes(), flate2::Compression::fast());
            gzhtml.clear();
            gz.read_to_end(&mut gzhtml).unwrap();

            // Write out re-compressed html file.
            zip_out
                .start_file_raw_name(name_raw, zip::write::FileOptions::default())
                .unwrap();
            zip_out.write_all(&gzhtml).unwrap();
        }
        // Everything else
        else {
            zip_out
                .start_file_raw_name(name_raw, zip::write::FileOptions::default())
                .unwrap();
            zip_out.write_all(&data).unwrap();
        }

        data.clear();
    }
    println!("\r");

    zip_out.finish().unwrap();

    return Ok(());
}

/// Get the entries about the dictionary entry from
/// our other loaded dictionaries.
fn find_entry<'a>(
    _word: &str,
    kana: &str,
    writings: &[String],
    jm_table: &'a HashMap<(String, String), Morph>,
    pa_table: &HashMap<(String, String), u32>,
) -> (Option<&'a Morph>, String, Option<u32>) // Morph, Kana, Pitch Accent
{
    let mut morph = None;
    let mut pitch_accent = None;

    // Definition.
    for w in writings {
        if let Some(m) = jm_table.get(&(w.clone(), kana.into())) {
            if !m.definitions.is_empty() {
                morph = Some(m);
                break;
            }
        }
    }

    // Pitch accent.
    for w in writings {
        if let Some(pa) = pa_table.get(&(w.clone(), kana.into())) {
            pitch_accent = Some(*pa);
            break;
        }
    }

    (morph, kana.into(), pitch_accent)
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

fn process_entries(
    inn: &str,
    out: &mut String,
    jm_table: &HashMap<(String, String), jmdict::Morph>,
    pa_table: &HashMap<(String, String), u32>,
) {
    let mut parser = Reader::from_str(inn);

    let mut state = PS::None;

    let re_writings = regex::Regex::new(r"【([^】]*)】").unwrap();

    let mut buf = Vec::new();
    while let Ok(event) = parser.read_event(&mut buf) {
        match event {
            Event::Eof => {
                break;
            }

            Event::Start(e) => {
                if let PS::Word(word) = state.clone() {
                    if e.name() == b"b" {
                        // Get the kana pronunciation.
                        let kana = if let Ok(Event::Text(e)) = parser.read_event(&mut buf) {
                            strip_non_kana(bytes_to_str(&e))
                        } else {
                            "".into()
                        };
                        let _ = parser.read_event(&mut buf); // Skip "</b>".

                        // Get the (probably kanji) writings.
                        let mut writings = Vec::new();
                        if let Ok(Event::Text(e)) = parser.read_event(&mut buf) {
                            let text = bytes_to_str(&e);
                            if let Some(cap) = re_writings.captures_iter(text).next() {
                                let tmp: Vec<_> = cap[1].split("／").map(|s| s.into()).collect();
                                writings.extend_from_slice(&tmp);
                            }
                        }
                        if !writings.contains(&word) {
                            writings.push(word.clone());
                        }

                        // Actually generate the new entry text.
                        out.push_str("<hr/>");
                        let (morph, kana, pitch_accent) = find_entry(
                            &word,
                            &hiragana_to_katakana(&kana),
                            &writings,
                            jm_table,
                            pa_table,
                        );
                        out.push_str(&generate_header_text(&kana, pitch_accent, &writings));
                        if let Some(ref m) = morph {
                            out.push_str(&generate_definition_text(m));
                        }

                        // Change states.
                        state = PS::None;
                    } else {
                        // Copy to the output.
                        out.push_str(&format!("<{}>", bytes_to_str(&e)));
                    }
                } else {
                    // Copy to the output.
                    out.push_str(&format!("<{}>", bytes_to_str(&e)));
                }
            }

            Event::Empty(e) => {
                // Copy to the output.
                out.push_str(&format!("<{}/>", bytes_to_str(&e)));

                // Check if it's a state change.
                if state == PS::None
                    && e.name() == b"a"
                    && e.attributes().count() > 0
                    && e.attributes().nth(0).unwrap().unwrap().key == b"name"
                {
                    state = PS::Word(bytes_to_string(
                        &e.attributes().nth(0).unwrap().unwrap().value,
                    ));
                }
            }

            Event::End(e) => {
                // Check if it's a state change.
                if e.name() == b"w" {
                    out.push_str("<br/>");
                    state = PS::None;
                }

                // Copy to the output.
                out.push_str(&format!("</{}>", bytes_to_str(&e)));
            }

            Event::Text(e) => {
                let text = bytes_to_str(&e);

                // Copy to the output.
                out.push_str(text);
            }

            Event::Comment(e) => {
                // Copy to the output.
                out.push_str(&format!("<!-- {} -->", bytes_to_str(&e)));
            }

            Event::CData(e) => {
                // Copy to the output.
                out.push_str(&format!("<![CDATA[{}]]>", bytes_to_str(&e)));
            }

            Event::Decl(e) => {
                // Copy to the output.
                out.push_str(&format!("<?xml {}?>", bytes_to_str(&e)));
            }

            Event::PI(e) => {
                // Copy to the output.
                out.push_str(&format!("<?{}?>", bytes_to_str(&e)));
            }

            Event::DocType(e) => {
                // Copy to the output.
                out.push_str(&format!("<!DOCTYPE {}>", bytes_to_str(&e)));
            }
        }
    }
}

/// Parse State (PS)
#[derive(Clone, Debug, PartialEq, Eq)]
enum PS {
    None,
    Word(String),
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
const KANA_DIFF: u32 = 0x30a0 - 0x3040;

/// Removes all non-kana text from a `&str`, and returns
/// a `String` of the result.
fn strip_non_kana(text: &str) -> String {
    let mut new_text = String::new();
    for ch in text.chars() {
        if (ch as u32 >= 0x3040 && ch as u32 <= 0x309f)
            || (ch as u32 >= 0x30a0 && ch as u32 <= 0x30ff)
        {
            new_text.push(ch);
        }
    }
    new_text
}

fn hiragana_to_katakana(text: &str) -> String {
    let mut new_text = String::new();
    for ch in text.chars() {
        new_text.push(if ch as u32 >= 0x3040 && ch as u32 <= 0x309f {
            char::try_from(ch as u32 + KANA_DIFF).unwrap_or(ch)
        } else {
            ch
        });
    }
    new_text
}

fn katakana_to_hiragana(text: &str) -> String {
    let mut new_text = String::new();
    for ch in text.chars() {
        new_text.push(if ch as u32 >= 0x30a0 && ch as u32 <= 0x30ff {
            char::try_from(ch as u32 - KANA_DIFF).unwrap_or(ch)
        } else {
            ch
        });
    }
    new_text
}

fn is_all_kana(text: &str) -> bool {
    let mut all_kana = true;
    for ch in text.chars() {
        all_kana |= ch as u32 >= 0x3040 && ch as u32 <= 0x30ff;
    }
    all_kana
}
