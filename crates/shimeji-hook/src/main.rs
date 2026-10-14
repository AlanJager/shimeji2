//! Agent hook. This binary links only `shimeji-proto`.
//!
//! Exit status is always 0. Stdout and stderr stay empty, including when the
//! app is not listening.

use std::io::{self, Read};

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let _ = std::panic::catch_unwind(|| {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut stdin = String::new();
        let _ = io::stdin().take(1024 * 1024).read_to_string(&mut stdin);
        let _ = shimeji_hook::run_from_process(&stdin, &args);
    });
}
