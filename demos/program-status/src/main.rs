use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(program_status::App::new())
}
