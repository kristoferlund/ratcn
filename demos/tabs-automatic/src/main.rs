use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(tabs_automatic::App::new())
}
