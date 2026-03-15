use clap::Parser;
use color_eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = apex_cli::Cli::parse();

    // Load config: explicit --config path > auto-discover from CWD > defaults.
    let cfg = match &cli.config {
        Some(path) => apex_core::config::ApexConfig::from_file(path)
            .map_err(|e| color_eyre::eyre::eyre!("{e}"))?,
        None => {
            apex_core::config::ApexConfig::discover(&match std::env::current_dir() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("error: cannot access current directory: {e}");
                    std::process::exit(1);
                }
            })
                .map_err(|e| color_eyre::eyre::eyre!("{e}"))?
        }
    };

    // Init tracing (only here, never in lib — it panics if called twice).
    let log_level = cli.log_level.as_deref().unwrap_or(&cfg.logging.level);
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(log_level))
        .with_writer(std::io::stderr)
        .init();

    apex_cli::run_cli(cli, &cfg).await
}
