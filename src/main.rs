use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use fan_control::config::Config;
use fan_control::controller::{self, Controller};
use fan_control::events::{EventLayer, EventLog};
use fan_control::history::History;
use fan_control::ipmi::IpmiTool;
use fan_control::state::AppState;
use fan_control::web;

/// Fan control for Supermicro motherboards through IPMI, with a web UI.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Configuration file (created with defaults if missing).
    #[arg(short, long, env = "FAN_CONTROL_CONFIG", default_value = "/etc/fan-control/config.toml", global = true)]
    config: PathBuf,

    /// Read sensors and compute speeds, but never write to the BMC.
    #[arg(short, long)]
    dry_run: bool,

    /// Fake BMC and sensors, to try the UI without a Supermicro server.
    #[arg(long)]
    simulate: bool,

    /// Overrides `web.listen` from the configuration.
    #[arg(long, env = "FAN_CONTROL_LISTEN", global = true)]
    listen: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Sets the web UI password (read from standard input).
    SetPassword,
    /// Validates the configuration file and exits.
    CheckConfig,
    /// Exits with 0 if the running service answers on its web port (Docker HEALTHCHECK).
    Healthcheck,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::SetPassword) => set_password(&cli.config),
        Some(Command::CheckConfig) => {
            Config::load_or_create(&cli.config)?;
            println!("{}: OK", cli.config.display());
            Ok(())
        }
        Some(Command::Healthcheck) => {
            let listen = match &cli.listen {
                Some(listen) => listen.clone(),
                None => Config::load(&cli.config)?.web.listen,
            };
            web::health::check(web::health::probe_address(&listen)?, std::time::Duration::from_secs(5))
        }
        None => tokio::runtime::Runtime::new()?.block_on(run(cli)),
    }
}

fn set_password(path: &std::path::Path) -> anyhow::Result<()> {
    let mut config = Config::load_or_create(path)?;
    eprintln!("New web UI password:");
    let mut password = String::new();
    std::io::stdin().lock().read_line(&mut password)?;
    let password = password.trim_end_matches(['\r', '\n']);
    anyhow::ensure!(
        password.chars().count() >= web::auth::MIN_PASSWORD_LEN,
        "the password needs at least {} characters",
        web::auth::MIN_PASSWORD_LEN
    );
    config.web.password_hash = web::auth::hash_password(password)?;
    config.save(path)?;
    eprintln!("Password saved. Restart the service to log out existing sessions.");
    Ok(())
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let events = EventLog::default();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_target(false).with_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("fan_control=info,warn")),
            ),
        )
        .with(EventLayer(events.clone()))
        .init();

    let config = Config::load_or_create(&cli.config)?;
    let history = if config.history.enabled {
        let path = controller::history_path(&cli.config);
        Some(History::open(&path).with_context(|| format!("cannot open history {}", path.display()))?)
    } else {
        None
    };
    let listen = cli.listen.clone().unwrap_or_else(|| config.web.listen.clone());
    let state = AppState::new(cli.config.clone(), config, history, events, cli.simulate, cli.dry_run);

    if cli.simulate {
        tracing::warn!("simulation mode: no real hardware is used");
    }
    if state.dry_run(&state.config()) {
        tracing::warn!("dry-run: fan speeds are computed but not applied");
    }
    if state.config().web.password_hash.is_empty() {
        tracing::warn!("no web UI password yet: open the web UI to create one");
    }
    install_panic_hook(state.clone());

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let control = tokio::spawn(Controller::new(state.clone()).run(shutdown_rx.clone()));

    let listener =
        tokio::net::TcpListener::bind(&listen).await.with_context(|| format!("cannot listen on {listen}"))?;
    tracing::info!("fan-control {} - web UI on http://{listen}", env!("CARGO_PKG_VERSION"));
    let mut server_shutdown = shutdown_rx.clone();
    let server = tokio::spawn(
        axum::serve(listener, web::router(state.clone()))
            .with_graceful_shutdown(async move {
                let _ = server_shutdown.changed().await;
            })
            .into_future(),
    );

    wait_for_signal().await;
    tracing::info!("stopping");
    let _ = shutdown_tx.send(true);
    // The controller restores the BMC fan mode before returning.
    control.await?;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
    Ok(())
}

/// On a crash, hand the fans back to the BMC so the server is not left at a fixed speed.
fn install_panic_hook(state: Arc<AppState>) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let config = state.config();
        if state.simulate || state.dry_run(&config) {
            return;
        }
        let mode = config.general.exit_fan_mode;
        if let Err(e) = IpmiTool::new(config.ipmi).set_fan_mode_blocking(mode) {
            eprintln!("cannot restore fan mode after panic: {e:#}");
        }
    }));
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
