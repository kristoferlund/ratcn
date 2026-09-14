// One component module, copied at build time and compiled alone. See build.rs.
include!(concat!(env!("OUT_DIR"), "/", env!("CARGO_BIN_NAME"), ".rs"));

fn main() {}
