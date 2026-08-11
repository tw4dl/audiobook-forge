#![forbid(unsafe_code)]

fn main() {
    if let Err(error) = kokoro_book::cli::run() {
        eprintln!(
            "error: {}",
            kokoro_book::cli::terminal_text(&format!("{error:#}"))
        );
        std::process::exit(1);
    }
}
