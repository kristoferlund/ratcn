use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(tabs_basic::App::new())
}
