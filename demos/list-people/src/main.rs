use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(list_people::App::new())
}
