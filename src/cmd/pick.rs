//! `kav pick`: print free ports a server can bind to.

use std::process::ExitCode;

use anyhow::Result;

use crate::cli::PortRange;
use crate::models::Protocol;
use crate::probe::{bind_check, BindCheck};
use crate::scanner::listening_ports;

pub struct PickArgs {
    pub range: PortRange,
    pub count: usize,
    pub udp: bool,
}

pub fn run(args: &PickArgs) -> Result<ExitCode> {
    let proto = if args.udp {
        Protocol::Udp
    } else {
        Protocol::Tcp
    };
    let listening = listening_ports(proto == Protocol::Tcp, proto == Protocol::Udp)?;
    let free: Vec<u16> = (args.range.start..=args.range.end)
        .filter(|&p| !listening.contains(&(proto, p)))
        .filter(|&p| bind_check(p, proto) == BindCheck::Free)
        .take(args.count)
        .collect();
    for port in &free {
        println!("{port}");
    }
    if free.len() < args.count {
        eprintln!(
            "Only {} of {} free {} port(s) found in {}-{}",
            free.len(),
            args.count,
            proto,
            args.range.start,
            args.range.end
        );
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}
