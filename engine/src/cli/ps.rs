// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii ps` and `iii kill`: the iii processes on this machine, and a way to
//! stop them by engine port, by compose project, or by being orphaned.

use std::{collections::BTreeSet, path::PathBuf, time::Duration};

use colored::Colorize;
use iii_compose::ps::{IiiProcess, Kind};

#[derive(clap::Args, Debug)]
#[command(group(
    clap::ArgGroup::new("target")
        .required(true)
        .args(["port", "compose", "orphans"])
))]
pub struct KillArgs {
    /// Stop the engine on this port, the compose daemon that manages it, and
    /// every worker pointed at it
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    /// Stop a compose project: its daemon, managed engine and workers. Takes
    /// the compose file or its directory [default: ./worker-compose.yaml]
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = ".")]
    pub compose: Option<PathBuf>,

    /// Stop every compose worker whose compose daemon is gone
    #[arg(long)]
    pub orphans: bool,

    /// Send SIGKILL at once instead of letting processes shut down
    #[arg(long)]
    pub force: bool,
}

/// How long each stage waits for a graceful exit before SIGKILL. A compose
/// daemon spends it stopping its project.
const GRACE: Duration = Duration::from_secs(15);

pub fn ps(json: bool) -> anyhow::Result<()> {
    let processes = iii_compose::ps::scan();
    if json {
        println!("{}", serde_json::to_string_pretty(&processes)?);
        return Ok(());
    }
    if processes.is_empty() {
        println!("no iii processes running");
        return Ok(());
    }

    let engines: BTreeSet<u16> = processes
        .iter()
        .filter(|process| process.kind == Kind::Engine)
        .flat_map(|process| process.ports.iter().copied())
        .collect();
    let now = now_secs();
    let rows: Vec<[String; 7]> = processes
        .iter()
        .map(|process| {
            [
                process.pid.to_string(),
                kind_label(process.kind).to_string(),
                process.name.clone().unwrap_or_else(|| "-".to_string()),
                ports(&process.ports),
                status(process, &engines).to_string(),
                uptime(now.saturating_sub(process.started_at)),
                project(process),
            ]
        })
        .collect();
    let header = ["PID", "KIND", "NAME", "PORT", "STATUS", "UPTIME", "COMPOSE"];
    let mut widths = header.map(str::len);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |cells: &[String; 7]| {
        cells
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    println!("{}", line(&header.map(str::to_string)).dimmed());
    for row in &rows {
        let text = line(row);
        if row[4] == "orphan" || row[4] == "no engine" {
            println!("{}", text.yellow());
        } else {
            println!("{text}");
        }
    }
    let orphans = processes.iter().filter(|process| process.orphan).count();
    if orphans > 0 {
        println!(
            "\n{} orphaned worker(s): their compose is gone. Stop them with {}",
            orphans,
            "iii kill --orphans".bold()
        );
    }
    Ok(())
}

pub async fn kill(args: &KillArgs) -> anyhow::Result<i32> {
    let me = std::process::id();
    let compose = match &args.compose {
        Some(path) => Some(compose_file(path)?),
        None => None,
    };
    let targets: Vec<IiiProcess> = iii_compose::ps::scan()
        .into_iter()
        .filter(|process| process.pid != me)
        .filter(|process| {
            args.port.is_some_and(|port| process.ports.contains(&port))
                || compose
                    .as_ref()
                    .is_some_and(|file| process.compose_file.as_ref() == Some(file))
                || (args.orphans && process.orphan)
        })
        .collect();

    if targets.is_empty() {
        eprintln!("no matching iii processes");
        return Ok(1);
    }
    for process in &targets {
        println!(
            "stopping {} {}(pid {})",
            kind_label(process.kind),
            process
                .name
                .as_deref()
                .map(|name| format!("{name} "))
                .unwrap_or_default(),
            process.pid
        );
    }

    let left = iii_compose::ps::stop(&targets, GRACE, args.force).await;
    if left.is_empty() {
        println!("{} {} process(es)", "stopped".green(), targets.len());
        Ok(0)
    } else {
        eprintln!(
            "{} still running: {}",
            "error:".red(),
            left.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(1)
    }
}

/// A compose file, or the directory that holds `worker-compose.yaml`, as the
/// canonical path compose records in its children's environment.
fn compose_file(path: &std::path::Path) -> anyhow::Result<PathBuf> {
    let file = if path.is_dir() {
        path.join("worker-compose.yaml")
    } else {
        path.to_path_buf()
    };
    file.canonicalize()
        .map_err(|error| anyhow::anyhow!("{}: {error}", file.display()))
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Engine => "engine",
        Kind::Compose => "compose",
        Kind::Worker => "worker",
    }
}

fn status(process: &IiiProcess, engines: &BTreeSet<u16>) -> &'static str {
    if process.orphan {
        "orphan"
    } else if process.kind == Kind::Worker
        && process.ports.iter().any(|port| !engines.contains(port))
    {
        "no engine"
    } else {
        "running"
    }
}

fn ports(ports: &[u16]) -> String {
    if ports.is_empty() {
        return "-".to_string();
    }
    ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// The project directory, with the home directory shortened to `~`.
fn project(process: &IiiProcess) -> String {
    let Some(file) = &process.compose_file else {
        return "-".to_string();
    };
    let dir = file.parent().unwrap_or(file);
    match dirs::home_dir().and_then(|home| dir.strip_prefix(home).ok().map(PathBuf::from)) {
        Some(relative) => format!("~/{}", relative.display()),
        None => dir.display().to_string(),
    }
}

fn uptime(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h{}m", secs / 3600, secs % 3600 / 60),
        _ => format!("{}d{}h", secs / 86400, secs % 86400 / 3600),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::uptime;

    #[test]
    fn uptime_reads_at_a_glance() {
        assert_eq!(uptime(5), "5s");
        assert_eq!(uptime(125), "2m");
        assert_eq!(uptime(3 * 3600 + 120), "3h2m");
        assert_eq!(uptime(2 * 86400 + 3600), "2d1h");
    }
}
