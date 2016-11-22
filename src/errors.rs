use cargo::CargoError;
use std::env::VarError;
use std::io;

error_chain!{
    foreign_links {
        Box<CargoError>, CargoErr;
        VarError, VarError;
        io::Error, IoError;
    }
}

