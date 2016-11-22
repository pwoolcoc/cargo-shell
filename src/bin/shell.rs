extern crate shell;
extern crate env_logger;

use std::io::{stderr, Write};
use std::process;
fn main() {
    env_logger::init().unwrap();
    if let Err(e) = shell::main() {
        let stderr = stderr();
        let _ = writeln!(stderr.lock(), "Error was {:?}", e);
        process::exit(255);
    }
}

