use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "mythterm", about = "GPU-accelerated terminal emulator")]
struct Args {
    #[arg(long, short = 'n')]
    skip_config: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    let _args = Args::parse();
    log::info!("mythterm starting");
    // TODO: initialize myth engine, font system, mux, UI
    Ok(())
}
