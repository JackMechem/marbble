//! Prints (or with `--reset`, restores to 100%) the system input volume for a recording device (Linux, macOS):
//! `--example input_level [--reset] [device-id]`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reset = args.iter().any(|a| a == "--reset");
    let id = args.iter().find(|a| !a.starts_with("--")).cloned();
    if reset {
        println!("{:?}", daw_engine::audio_io::system_volume::reset_input_level(id.as_deref()));
    } else {
        println!("{:?}", daw_engine::audio_io::system_volume::input_level(id.as_deref()));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("input_level reads the system input volume, which Marbble checks on Linux and macOS only");
}
