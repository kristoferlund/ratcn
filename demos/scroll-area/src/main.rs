use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(scroll_area::App::new())
}
