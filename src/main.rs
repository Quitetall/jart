//! `jart` binary — Just Another Research Tool. Thin wrapper over `jart::cli::run`
//! (shared with the `research` alias binary in `src/bin/research.rs`).

fn main() -> anyhow::Result<()> {
    jart::cli::run()
}
