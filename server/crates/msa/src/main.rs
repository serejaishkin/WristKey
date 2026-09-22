use tracing::info;
use wristkey_msa::run;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wristkey_msa=debug,tower_http=warn".into()),
        )
        .init();

    let listen = std::env::var("WRISTKEY_MSA_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_owned());
    let token = std::env::var("WRISTKEY_MSA_TOKEN").ok();
    wristkey_msa::configure_auth(token);
    info!("starting wristkey-msa on {listen}");
    run(&listen).await?;
    Ok(())
}
