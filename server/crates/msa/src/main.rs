use tracing::info;
use wristkey_msa::{run, Config};

fn take(args: &[String], i: &mut usize, name: &str) -> Option<String> {
    let a = args.get(*i)?.as_str();
    if let Some(v) = a.strip_prefix(&format!("{name}=")) {
        return Some(v.to_owned());
    }
    if a == name {
        *i += 1;
        return args.get(*i).cloned();
    }
    None
}

fn parse_args(args: Vec<String>) -> (Option<String>, Option<String>, Option<String>) {
    let mut mode = None;
    let mut listen = None;
    let mut token = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].clone();
        if let Some(v) = take(&args, &mut i, "--mode") {
            mode = Some(v);
        } else if let Some(v) = take(&args, &mut i, "--listen") {
            listen = Some(v);
        } else if let Some(v) = take(&args, &mut i, "--token") {
            token = Some(v);
        } else if a == "--help" || a == "-h" {
            eprintln!(
                "Usage: wristkey-msa [--mode local|lan] [--listen ADDR] [--token TOKEN]\n\
                 \n\
                 Modes:\n\
                   local  bind 127.0.0.1:8787 (default); token optional\n\
                   lan    bind 0.0.0.0:8787; token required\n\
                 \n\
                 Flags override WRISTKEY_MSA_MODE / WRISTKEY_MSA_LISTEN / WRISTKEY_MSA_TOKEN."
            );
            std::process::exit(0);
        } else {
            eprintln!("unknown argument: {a}");
            std::process::exit(2);
        }
        i += 1;
    }
    (mode, listen, token)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wristkey_msa=debug,tower_http=warn".into()),
        )
        .init();

    let (cli_mode, cli_listen, cli_token) = parse_args(std::env::args().skip(1).collect());
    let cfg = Config::resolve(cli_mode, cli_listen, cli_token)?;
    wristkey_msa::configure_auth(cfg.token);
    info!("starting wristkey-msa in {} mode on {}", cfg.mode, cfg.listen);
    run(&cfg.listen).await?;
    Ok(())
}