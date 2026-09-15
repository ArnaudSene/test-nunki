use std::env;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    ExitCode::from(rgrep::run(&args, &mut stdout, &mut stderr))
}
