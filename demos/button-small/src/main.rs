use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(button_small::App::new())
}
