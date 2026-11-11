use std::io;

fn main() -> io::Result<()> {
    // `--mute` plays the film without its soundtrack, and `--autoplay` plays
    // it straight through instead of waiting at the end of each section.
    let flag = |name: &str| std::env::args().any(|arg| arg == name);
    demo_shared::run(ui_f_state::App::new(!flag("--mute"), !flag("--autoplay")))
}
