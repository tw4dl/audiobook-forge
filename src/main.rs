fn main() {
    if let Err(error) = kokoro_book::cli::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
