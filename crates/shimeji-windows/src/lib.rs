//! Windows shell; Win32 calls are confined to the Windows-only module.

pub mod focus;
pub mod launch;
pub mod sprites;

#[cfg(target_os = "windows")]
mod native;

pub fn run() -> std::process::ExitCode {
    #[cfg(target_os = "windows")]
    {
        match native::run() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                native::show_error(&error);
                std::process::ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("shimeji2 Windows shell requires Windows 10 or later");
        std::process::ExitCode::from(2)
    }
}
