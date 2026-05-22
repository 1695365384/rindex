use anyhow::Result;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rindex=info".parse()?)
        )
        .init();

    tracing::info!("rindex starting...");
    Ok(())
}
