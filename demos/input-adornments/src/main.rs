use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(input_adornments::App::new())
}
