use std::io;

fn main() -> io::Result<()> {
    // `--mute` plays the film without its soundtrack. `--pause <seconds>` sets
    // how long it stands still at the end of each section; `--pause 0` plays
    // it straight through.
    let arguments: Vec<String> = std::env::args().collect();
    let sound = !arguments.iter().any(|argument| argument == "--mute");
    let pause = arguments
        .iter()
        .position(|argument| argument == "--pause")
        .and_then(|flag| arguments.get(flag + 1)?.parse::<f64>().ok())
        .map_or(ui_f_state::PAUSE, |seconds| seconds.clamp(0.0, 60.0));
    demo_shared::run(ui_f_state::App::new(sound, pause))
}
