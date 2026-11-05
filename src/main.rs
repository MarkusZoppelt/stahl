//! Stahl Capital, the executable. See the library for everything else.

use std::env;
#[cfg(target_os = "linux")]
use std::process;

use tokio::runtime::Builder;

use stahl::{app, backend::Backend};

fn main() {
    #[cfg(target_os = "linux")]
    if env::var_os("DISPLAY").is_none() && env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!(
            "error: no display server detected \
             (DISPLAY and WAYLAND_DISPLAY are both unset).\n\
             Stahl is a desktop GUI — run it from a graphical session."
        );
        process::exit(1);
    }

    // GPUI owns the main thread; portfolio_rs runs on Tokio beside it.
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the Tokio runtime");
    let backend = Backend::load(runtime.handle().clone());
    let initial_file = env::args().skip(1).find(|arg| !arg.starts_with('-'));

    app::run(backend, initial_file);
}
