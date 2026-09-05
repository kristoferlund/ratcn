use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(ledger93::App::new())
}
