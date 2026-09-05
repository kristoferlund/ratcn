use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(wizard::App::new())
}
