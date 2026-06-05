//! `research` binary — alias for `jart` (same tool, legacy name). Both call the
//! shared `jart::cli::run` entrypoint.

fn main() -> anyhow::Result<()> {
    jart::cli::run()
}
