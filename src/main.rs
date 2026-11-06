//! Stahl Capital, the executable. See the library for everything else.

use std::env;
#[cfg(target_os = "linux")]
use std::process;

use tokio::runtime::Builder;

use stahl::{app, backend::Backend, state::initial_file};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (file, rune_args) = initial_file(&args);

    #[cfg(target_os = "linux")]
    if rune_args.is_empty()
        && env::var_os("DISPLAY").is_none()
        && env::var_os("WAYLAND_DISPLAY").is_none()
    {
        eprintln!(
            "error: no display server detected \
             (DISPLAY and WAYLAND_DISPLAY are both unset).\n\
             Stahl is a desktop GUI — run it from a graphical session."
        );
        process::exit(1);
    }

    // Rune owns the main thread; portfolio_rs runs on Tokio beside it.
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the Tokio runtime");
    let backend = Backend::load(runtime.handle().clone());

    app::run(backend, file, rune_args);
}
