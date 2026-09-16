mod adapters;
pub mod commands;
mod db;
pub mod domain;
pub mod error;
mod infra;
mod ports;
pub mod runtime;
pub mod tools;
pub mod ui;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();
    let context = runtime::AppContext::discover()?;
    ui::run(context)?;
    Ok(())
}
