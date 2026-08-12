#![forbid(unsafe_code)]

fn main() {
    if let Err(error) = audiobook_forge::cli::run() {
        eprintln!(
            "error: {}",
            audiobook_forge::cli::terminal_text(&format!("{error:#}"))
        );
        std::process::exit(1);
    }
}
