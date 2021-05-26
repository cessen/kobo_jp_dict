# Kobo Japanese Dictionary Builder

A tool that generates Japanese-English dictionaries for the [Kobo](https://www.kobo.com) line of e-readers.

It requires a [JMDict](https://www.edrdg.org/wiki/index.php/JMdict-EDICT_Dictionary_Project) XML file as input, and can optionally take pitch accent data and Japanese-Japanese definitions from certain sources as well.


## Example usage

Basic usage looks like this:

```
kobo_dict -j JMdict_e.xml -p accent.tsv dicthtml-ja-en.zip
```

This takes `JMdict_e.xml` and the pitch-accent file `accent.tsv` as input, and produces the Kobo dictionary file `dicthtml-ja-en.zip`.


## Installing the produced dictionary

On recent Kobo firmware the installation process is very straightforward: just copy the produced dictionary file to `.kobo/custom-dict/dicthtml-ja-en.zip` on your Kobo device.

Note that the filename is important: Kobo e-readers use the dictionary filename to determine the type of dictionary and what language(s) it's for.  Your Kobo may fail to register the dictionary if you name it differently.


## Using the dictionary

After installation, you can use it just like any other dictionary on the Kobo.  It will show up as `日本語 - English (Custom)` in your Kobo's dictionary drop-down list.

The dictionary entries look roughly like this (as best I can approximate with markdown):

> たべる [2]&nbsp;&nbsp;  — 【食べる／喰べる】 *verb, ichidan, transitive*
>
> 1. to eat
> 2. to live on (e.g. a salary); to live off; to subsist on

The entry header (at the top) consists of four parts in this order:

1. **Pronunciation** in hiragana.
2. **Pitch accent**, enclosed in square brackets. This will be absent if you didn't provide a pitch-accent file or if the word wasn't in the pitch accent file.
3. **Written forms**, enclosed in fancy square brackets. Generally the more common forms are listed first.
4. **Grammatical information**, in a comma separated list. This is always present for verbs and i-adjectives, but otherwise is (intentionally) typically absent. The rationale for this minimalism is that 1. this is a reading-oriented dictionary, and 2. most of the remaining grammatical information is fairly obvious from context or from the translations/definitions.

After the entry header is a numbered list of translations/definitions, generally with more common usages closer to the top.


## Requirements

To build, you just need a standard installation of [Rust](https://www.rust-lang.org).  You can then build this project with the typical `cargo build --release` command.

To run, you also need:

- A good bit of free RAM (around 2GB).  It deals with a lot of data, and I put zero effort into making it memory efficient because I don't expect it to be run frequently.
- The `marisa-build` executable from the [Marisa Trie project](https://github.com/s-yata/marisa-trie) installed and in your path.


## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.


## Contributing

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you will be licensed as above, without any
additional terms or conditions.