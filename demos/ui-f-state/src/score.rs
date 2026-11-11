//! When things happen. The picture and the soundtrack are both functions of
//! the film clock, and these are the moments they agree on.

/// One bar of the music, at 96 beats a minute. Every scene starts on one.
pub const BAR: f64 = 2.5;
pub const BEAT: f64 = BAR / 4.0;

/// The old way: every widget with its own state.
pub const CHAOS: f64 = 7.5;
/// The click that mutates one widget in place.
pub const CHAOS_CLICK: f64 = 9.0;
/// It blows up, and the title begins.
pub const BOOM: f64 = 17.5;
/// The title's letters land.
pub const TITLE_LANDS: f64 = 21.25;
pub const MONK: f64 = 25.0;
pub const RAT: f64 = 30.0;
pub const NINJA: f64 = 35.0;
/// The pointer presses the button the ninja answers.
pub const NINJA_CLICK: f64 = 36.25;
pub const NINJA_SLASH: f64 = 36.875;
/// The loop: state, render, input, update.
pub const LOOP: f64 = 40.0;
/// What it makes possible: four beats of [`POWER`] seconds.
pub const POWERS: f64 = 55.0;
pub const POWER: f64 = 5.0;
pub const FINALE: f64 = 75.0;
/// The title comes back.
pub const FINALE_DROP: f64 = 77.5;
/// The film is over, and the screen is handed to the viewer.
pub const END: f64 = 85.0;

/// Where the film waits to be continued: the last moment of each section,
/// with everything on screen and nothing yet fading. The loop waits twice,
/// once after its first, slow turn and once at its end; the old way waits
/// just short of the frames that flicker, which are not ones to sit in.
pub const HOLDS: [f64; 11] = [
    BOOM - 0.7,
    MONK - 0.35,
    RAT - 0.3,
    NINJA - 0.3,
    LOOP - 0.3,
    TURNS[1] - 0.05,
    POWERS - 0.3,
    POWERS + POWER - 0.3,
    POWERS + 2.0 * POWER - 0.3,
    POWERS + 3.0 * POWER - 0.3,
    FINALE - 0.3,
];

/// When each turn of the loop starts. The last entry is where the last ends.
pub const TURNS: [f64; 10] = [
    41.25, 45.0, 47.5, 50.0, 51.25, 52.5, 53.125, 53.75, 54.375, 55.0,
];
/// How far through a turn `update` writes the state.
pub const UPDATE_AT: f64 = 0.45;
/// How far through a turn the new frame is on screen.
pub const REPAINT_AT: f64 = 0.9;
/// The count before the first press.
pub const FIRST_COUNT: u32 = 3;

/// Which turn of the loop `t` is in, and how far through it.
pub fn turn(t: f64) -> Option<(usize, f64)> {
    TURNS
        .windows(2)
        .position(|turn| t >= turn[0] && t < turn[1])
        .map(|index| {
            (
                index,
                (t - TURNS[index]) / (TURNS[index + 1] - TURNS[index]),
            )
        })
}

/// How many turns have passed the point `at` by `t`.
fn turns_past(t: f64, at: f64) -> u32 {
    TURNS
        .windows(2)
        .filter(|turn| t >= turn[0] + (turn[1] - turn[0]) * at)
        .count() as u32
}

/// The count the state holds at `t`.
pub fn state_count(t: f64) -> u32 {
    FIRST_COUNT + turns_past(t, UPDATE_AT)
}

/// The count on screen at `t`: the state's, one repaint behind.
pub fn shown_count(t: f64) -> u32 {
    FIRST_COUNT + turns_past(t, REPAINT_AT)
}

/// The film clock at which tape position `index` is shown in the rewind
/// beat, as `(seconds into the beat, position)` keyframes.
pub const REWIND: [(f64, f64); 8] = [
    (0.0, 9.0),
    (1.0, 9.0),
    (2.2, 0.0),
    (2.6, 0.0),
    (3.2, 6.0),
    (3.5, 6.0),
    (3.9, 2.0),
    (4.5, 9.0),
];

/// Where the tape's playhead is, `local` seconds into the rewind beat.
pub fn playhead(local: f64) -> f64 {
    REWIND
        .windows(2)
        .find(|pair| local < pair[1].0)
        .map_or(REWIND[REWIND.len() - 1].1, |pair| {
            let amount = ((local - pair[0].0) / (pair[1].0 - pair[0].0)).clamp(0.0, 1.0);
            pair[0].1 + (pair[1].1 - pair[0].1) * amount
        })
}
