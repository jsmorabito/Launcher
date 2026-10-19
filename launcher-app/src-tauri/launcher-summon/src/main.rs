use std::process::ExitCode;

fn main() -> ExitCode {
    match launcher_summon::run(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("launcher-summon: {error}");
            ExitCode::FAILURE
        }
    }
}
