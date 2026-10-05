use std::io;

fn main() -> io::Result<()> {
    #[cfg(target_arch = "wasm32")]
    return demo_shared::run_dom(landing::App::new());

    #[cfg(not(target_arch = "wasm32"))]
    demo_shared::run(landing::App::new())
}
