use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(landing::App::new())
}
