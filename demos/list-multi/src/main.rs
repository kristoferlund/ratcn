use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(list_multi::App::new())
}
