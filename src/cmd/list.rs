//! `kav list`: a one-shot table or JSON document, or a live `--watch` view.

use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use ratatui::crossterm::{
    cursor, queue,
    terminal::{Clear, ClearType},
};

use crate::models::{filter_entries, sort_entries, EntryKey, PortEntry, SortField};
use crate::output::{self, json, table, ColorChoice};
use crate::scanner::{ContainerMode, ScanOptions, Scanner};

pub struct ListArgs {
    pub json: bool,
    pub sort: SortField,
    pub reverse: bool,
    pub filter: Option<String>,
    pub watch: bool,
    pub interval: Duration,
    pub tcp: bool,
    pub udp: bool,
    pub exposed: bool,
}

fn scan_options(args: &ListArgs, sample_cpu: bool) -> ScanOptions {
    ScanOptions {
        tcp: args.tcp,
        udp: args.udp,
        sample_cpu,
        containers: ContainerMode::Blocking,
    }
}

/// Apply `--sort`/`--reverse`, `--filter` and `--exposed`.
fn select(mut entries: Vec<PortEntry>, args: &ListArgs) -> Vec<PortEntry> {
    sort_entries(&mut entries, args.sort, args.reverse);
    if let Some(pattern) = &args.filter {
        filter_entries(&mut entries, pattern);
    }
    if args.exposed {
        entries.retain(PortEntry::is_exposed);
    }
    entries
}

pub fn run(args: &ListArgs, color: ColorChoice) -> Result<()> {
    let mut scanner = Scanner::new();
    if args.watch {
        return watch(&mut scanner, args, color);
    }
    let snapshot = scanner.scan(&scan_options(args, true))?;
    let entries = select(snapshot.entries, args);
    let mut out = io::stdout().lock();
    if args.json {
        json::write_pretty(
            &json::list_doc(&entries, snapshot.visibility, None),
            &mut out,
        )?;
    } else {
        let opts = table::TableOpts {
            color: output::color_enabled(color, io::stdout().is_terminal()),
            width: output::terminal_width(),
            mark_new: &HashSet::new(),
        };
        table::print_list(&entries, snapshot.visibility, &opts, &mut out)?;
    }
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Refresh until interrupted. On a terminal each frame is redrawn in place
/// (cursor home, then clear what the previous frame left); otherwise frames
/// are appended, separated by a blank line. `--json` writes one JSON document
/// per line (NDJSON). Rows that appeared since the previous frame get a `+`.
fn watch(scanner: &mut Scanner, args: &ListArgs, color: ColorChoice) -> Result<()> {
    let tty = io::stdout().is_terminal();
    let color = output::color_enabled(color, tty);
    let mut previous: Option<HashSet<EntryKey>> = None;
    loop {
        let snapshot = scanner.scan(&scan_options(args, previous.is_none()))?;
        let entries = select(snapshot.entries, args);
        let keys: HashSet<EntryKey> = entries.iter().map(PortEntry::key).collect();
        let new: HashSet<EntryKey> = previous
            .as_ref()
            .map(|prev| keys.difference(prev).copied().collect())
            .unwrap_or_default();

        let mut out = io::stdout().lock();
        if args.json {
            json::write_line(
                &json::list_doc(&entries, snapshot.visibility, Some(unix_now())),
                &mut out,
            )?;
        } else {
            let mut frame = Vec::new();
            writeln!(
                frame,
                "kav — refreshing every {}s — Ctrl+C to exit\n",
                args.interval.as_secs_f64()
            )?;
            let opts = table::TableOpts {
                color,
                width: if tty { output::terminal_width() } else { None },
                mark_new: &new,
            };
            table::print_list(&entries, snapshot.visibility, &opts, &mut frame)?;
            if tty {
                // Clear the rest of each line so a shorter row leaves no residue.
                let frame = String::from_utf8_lossy(&frame).replace('\n', "\x1b[K\n");
                queue!(out, cursor::MoveTo(0, 0))?;
                out.write_all(frame.as_bytes())?;
                queue!(out, Clear(ClearType::FromCursorDown))?;
            } else {
                out.write_all(&frame)?;
                writeln!(out)?;
            }
            out.flush()?;
        }
        previous = Some(keys);
        std::thread::sleep(args.interval);
    }
}
