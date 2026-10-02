//! Differential parity driver matching `tools/DRIVER_FORMAT.md`.

use std::env;
use std::fs;
use std::process;
use zopfli_core::{compress, Format, Options};

const MAX_INPUT: usize = 4096;

fn want_section(sections: &[String], name: &str) -> bool {
    sections.is_empty() || sections.iter().any(|s| s == name)
}

fn print_hex(data: &[u8]) {
    for b in data {
        print!("{b:02x}");
    }
    println!();
}

fn emit(name: &str, format: Format, opt: &Options, input: &[u8]) -> Result<(), i32> {
    let out = compress(opt, format, input).map_err(|_| 1)?;
    if out.is_empty() {
        eprintln!("compress failed for {name}");
        return Err(1);
    }
    println!("=== {name} ===");
    println!("len {}", out.len());
    print!("hex ");
    print_hex(&out);
    Ok(())
}

fn main() {
    let mut args = env::args().skip(1);
    let mut sections: Vec<String> = Vec::new();
    let mut path: Option<String> = None;

    while let Some(arg) = args.next() {
        if arg == "--sections" {
            let Some(val) = args.next() else {
                eprintln!("missing --sections value");
                process::exit(2);
            };
            sections = val.split(',').map(str::to_string).collect();
        } else if path.is_none() {
            path = Some(arg);
        } else {
            eprintln!("unexpected arg {arg}");
            process::exit(2);
        }
    }

    let Some(path) = path else {
        eprintln!("usage: zopfli-driver [--sections a,b] <fixture>");
        process::exit(2);
    };

    let input = match fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("cannot open {path}");
            process::exit(2);
        }
    };

    if input.len() > MAX_INPUT {
        println!("SKIP oversize {}", input.len());
        process::exit(0);
    }

    let mut opt = Options::default();
    opt.numiterations = 1;
    opt.verbose = 0;
    opt.verbose_more = 0;

    let mut rc = 0;
    if want_section(&sections, "gzip") {
        if emit("gzip", Format::Gzip, &opt, &input).is_err() {
            rc = 1;
        }
    }
    if rc == 0 && want_section(&sections, "zlib") {
        if emit("zlib", Format::Zlib, &opt, &input).is_err() {
            rc = 1;
        }
    }
    if rc == 0 && want_section(&sections, "deflate") {
        if emit("deflate", Format::Deflate, &opt, &input).is_err() {
            rc = 1;
        }
    }
    process::exit(rc);
}
