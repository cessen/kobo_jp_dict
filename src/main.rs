use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::{BufReader, BufWriter};

use flate2::read::{GzDecoder, GzEncoder};
use quick_xml::{events::Event, Reader};

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
        .get_matches();

    // Open the input zip archive.
    let input_filename = matches.value_of("INPUT").unwrap();
    let mut zip_in = zip::ZipArchive::new(BufReader::new(File::open(input_filename)?))?;

    // Open the output zip archive.
    let output_filename = matches.value_of("OUTPUT").unwrap();
    let mut zip_out = zip::ZipWriter::new(BufWriter::new(File::create(output_filename)?));

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
            process_entries(&html, &mut html_processed);

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

/// The meat of the thing, used below to add additional
/// definition text to a word's entry.
fn generate_entry_new_text(word: &str) -> String {
    format!("YARBLE!  This is test 2.  {}  BLAH BLAH!<br/>", word)
}

fn process_entries(inn: &str, out: &mut String) {
    let mut parser = Reader::from_str(inn);

    let mut state = PS::None;

    let mut buf = Vec::new();
    while let Ok(event) = parser.read_event(&mut buf) {
        match event {
            Event::Eof => {
                break;
            }

            Event::Start(e) => {
                // Copy to the output.
                out.push_str(&format!("<{}>", bytes_to_str(&e)));

                // Check if it's a state change.
                if let PS::Word(ref word) = state {
                    // Check if it's the place where we should add
                    // in our own content.
                    if e.name() == b"p" {
                        // Put our own definition bits here.
                        out.push_str(&generate_entry_new_text(word));
                    }
                }
                if e.name() == b"a"
                    && e.attributes().count() > 0
                    && e.attributes().nth(0).unwrap().unwrap().key == b"name"
                {
                    state = PS::Word(bytes_to_string(
                        &e.attributes().nth(0).unwrap().unwrap().value,
                    ));
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
                // Copy to the output.
                out.push_str(&format!("</{}>", bytes_to_str(&e)));

                // Check if it's a state change.
                if e.name() == b"w" {
                    state = PS::None;
                }
            }

            Event::Text(e) => {
                // Copy to the output.
                out.push_str(bytes_to_str(&e));
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
