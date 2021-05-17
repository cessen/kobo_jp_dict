use flate2::read::{GzDecoder, GzEncoder};
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::{BufReader, BufWriter};

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
    let mut gzhtml = Vec::new();
    for i in 0..zip_in.len() {
        print!("\r{}/{}", i + 1, zip_in.len());
        let mut f = zip_in.by_index(i).unwrap();
        f.read_to_end(&mut data).unwrap();
        let name_raw = f.name_raw();

        // println!("{:0x?}", name_raw);

        // HTML files
        if name_raw.len() >= 5 && &name_raw[(name_raw.len() - 5)..] == &b".html"[..] {
            // Decompress html data.
            let mut ungz = GzDecoder::new(&data[..]);
            html.clear();
            ungz.read_to_string(&mut html).unwrap();

            // Recompress html data.
            let mut gz = GzEncoder::new(html.as_bytes(), flate2::Compression::fast());
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
