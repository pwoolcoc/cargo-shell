extern crate shell;
extern crate env_logger;
#[macro_use] extern crate error_chain;

use std::io::{self, Write};
use std::process;
use std::error::Error as StdError;

fn main() {
    env_logger::init().unwrap();
    if let Err(e) = shell::main() {
        let _ = writeln!(&mut io::stderr(), "{}", e.description());
        process::exit(1);
    }
}

