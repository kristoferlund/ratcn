use std::io;

fn main() -> io::Result<()> {
    demo_shared::run(toast::App::new())
}
