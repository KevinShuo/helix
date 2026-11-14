//! Portable external formatter for save/quit integration tests.
use std::io::{self, Read, Write};

fn main() {
    let mut args = std::env::args_os().skip(1);
    let marker = args.next().unwrap();
    let fail = args.next().unwrap() == "fail";
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    std::fs::write(marker, input).unwrap();
    io::stdout().write_all(b"new content\n").unwrap();
    io::stdout().flush().unwrap();
    if fail {
        std::process::exit(1);
    }
}
