use std::io;

fn main() -> io::Result<()> {
    // `--mute` plays the film without its soundtrack.
    let sound = !std::env::args().any(|arg| arg == "--mute");
    demo_shared::run(ui_f_state::App::new(sound))
}
