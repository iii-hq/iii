// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// See LICENSE and PATENTS files for details.

use clap::Parser;
use iii::{
    EngineBuilder, logging,
    modules::config::{DEFAULT_PORT, EngineConfig},
};

#[derive(Parser, Debug)]
#[command(name = "engine", about = "Process communication engine")]
struct Args {
    #[arg(short, long, default_value = "config.yaml")]
    config: String,
    #[arg(short = 'v', long)]
    version: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    logging::init_log(&args.config);

    let config = EngineConfig::config_file_or_default(&args.config)?;
    let port = if config.port == 0 {
        DEFAULT_PORT
    } else {
        config.port
    };

    EngineBuilder::new()
        .config_file_or_default(&args.config)?
        .address(format!("0.0.0.0:{}", port).as_str())
        .build()
        .await?
        .serve()
        .await?;
    Ok(())
}
