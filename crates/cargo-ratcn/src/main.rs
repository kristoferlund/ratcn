fn main() {
    if let Err(error) = cargo_ratcn::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
