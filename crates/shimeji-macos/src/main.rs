//! macOS shell binary. objc2 is linked only when building for macOS.

fn main() -> std::process::ExitCode {
    shimeji_macos::run()
}
