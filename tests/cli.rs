use std::process::Command;

mod common;
use common::{kav, Helper};

/// Re-exec target for [`Helper::spawn`]; a no-op in normal test runs.
#[test]
fn helper_process() {
    common::run_helper_from_env();
}

fn stdout_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Parse a JSON document printed by kav.
fn json_of(out: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("invalid JSON ({e}):\n{}", stdout_of(out)))
}

/// The `ports[]` objects of a `kav list --json` document for `port`.
fn json_ports(doc: &serde_json::Value, port: u16) -> Vec<serde_json::Value> {
    doc["ports"]
        .as_array()
        .expect("ports array")
        .iter()
        .filter(|p| p["port"] == port)
        .cloned()
        .collect()
}

/// CPU% of the TCP listener on `port` in a `kav list --json` document.
fn json_cpu_for_port(doc: &serde_json::Value, port: u16) -> Option<f64> {
    json_ports(doc, port)
        .iter()
        .find(|p| p["protocol"] == "TCP")
        .and_then(|p| p["cpu"].as_f64())
}

#[test]
fn list_json_v2_for_helper() {
    let helper = Helper::spawn("tcp");
    let doc = json_of(&kav(&["list", "--json"]));
    assert_eq!(doc["schema"], 2);
    let rows = json_ports(&doc, helper.port);
    assert_eq!(rows.len(), 1, "one TCP row for the helper: {rows:?}");
    let row = &rows[0];
    assert_eq!(row["pid"], helper.pid);
    assert_eq!(row["addresses"], serde_json::json!(["127.0.0.1"]));
    assert_eq!(row["exposed"], false);
    assert!(
        !row["command"].as_array().unwrap().is_empty(),
        "command line present: {row}"
    );
}

#[test]
fn exposed_helper_is_exposed() {
    let helper = Helper::spawn("exposed");
    let doc = json_of(&kav(&["list", "--json"]));
    let rows = json_ports(&doc, helper.port);
    assert_eq!(rows[0]["exposed"], true);
    assert_eq!(rows[0]["addresses"], serde_json::json!(["0.0.0.0"]));
}

#[test]
fn check_reports_helper_pid() {
    let helper = Helper::spawn("tcp");
    let out = kav(&["check", &helper.port.to_string()]);
    assert!(out.status.success(), "check should find the helper");
    assert!(
        stdout_of(&out).contains(&helper.pid.to_string()),
        "helper pid {} missing from:\n{}",
        helper.pid,
        stdout_of(&out)
    );
}

#[test]
fn list_cpu_reflects_busy_process() {
    let helper = Helper::spawn("busy");
    std::thread::sleep(std::time::Duration::from_millis(500));
    // The helper can be briefly starved while the rest of the suite starts up, so
    // take the best of three samples. A sampling bug reads <10% on every attempt.
    let mut readings = Vec::new();
    for _ in 0..3 {
        let out = kav(&["list", "--json"]);
        let cpu = json_cpu_for_port(&json_of(&out), helper.port)
            .unwrap_or_else(|| panic!("busy helper missing from JSON:\n{}", stdout_of(&out)));
        if cpu >= 30.0 {
            return;
        }
        readings.push(cpu);
    }
    panic!("busy helper spins a full core but kav reported {readings:?}% CPU");
}

#[test]
fn check_shows_command_line() {
    let helper = Helper::spawn("tcp");
    let out = kav(&["check", &helper.port.to_string()]);
    assert!(
        stdout_of(&out).contains("cmd:"),
        "check output should include the command line:\n{}",
        stdout_of(&out)
    );
}

#[test]
fn tcp_and_udp_on_same_port_are_both_listed() {
    let helper = Helper::spawn("tcpudp");
    let port = helper.port.to_string();
    let out = kav(&["check", &port]);
    let text = stdout_of(&out);
    let has = |proto: &str| text.lines().any(|l| l.contains(&port) && l.contains(proto));
    assert!(has("TCP"), "TCP row missing:\n{text}");
    assert!(has("UDP"), "UDP row missing:\n{text}");
}

#[test]
fn cli_list_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .arg("list")
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn cli_list_json_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let doc = json_of(&output);
    assert_eq!(doc["schema"], 2);
    assert!(doc["ports"].is_array());
}

#[test]
fn cli_list_sort_cpu() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--sort", "cpu"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn cli_check_unused_port_exits_1() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["check", "1"])
        .output()
        .unwrap();
    // Port 1 should not be in use — exit code 1
    assert!(!output.status.success());
}

#[test]
fn cli_check_multiple_ports() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["check", "1", "2", "3"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show summary for multiple ports
    assert!(stdout.contains("of 3 ports active"));
}

#[test]
fn cli_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kav"));
}

#[test]
fn cli_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Guard your ports"));
}

#[test]
fn cli_invalid_sort_value() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--sort", "invalid"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn cli_list_filter_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--filter", "postgres"])
        .output()
        .unwrap();
    // Command succeeds regardless of whether postgres is running
    assert!(output.status.success());
}

#[test]
fn cli_list_filter_short_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "-f", "redis"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn cli_list_filter_with_sort() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--filter", "node", "--sort", "cpu"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn cli_list_filter_numeric_is_exact_port() {
    // "80" as a valid u16 must exact-match port 80, not substring-match 8080/800/etc.
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--json", "--filter", "80"])
        .output()
        .unwrap();
    assert!(output.status.success());
    for row in json_of(&output)["ports"].as_array().unwrap() {
        assert_eq!(row["port"], 80, "filter '80' matched a non-80 port: {row}");
    }
}

#[test]
fn cli_list_filter_json_combo() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--json", "--filter", "zzzunlikelyservice"])
        .output()
        .unwrap();
    assert!(output.status.success());
    // No match → empty ports array
    assert_eq!(json_of(&output)["ports"], serde_json::json!([]));
}

#[test]
fn cli_man_outputs_roff() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .arg("man")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Man pages start with .TH header
    assert!(stdout.contains(".TH"), "man page should contain .TH macro");
    assert!(stdout.contains("kav"), "man page should mention kav");
}

#[test]
fn cli_kill_nonexistent_port_no_prompt() {
    // Port 2 is virtually never in use; --yes bypasses TTY prompt
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["kill", "--yes", "2"])
        .output()
        .unwrap();
    // Should exit 1 (nothing found) without hanging on prompt
    assert!(!output.status.success());
}

#[test]
fn cli_list_watch_flag_in_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--watch") || stdout.contains("-w"),
        "--watch flag should appear in list help"
    );
}

#[test]
fn cli_list_tcp_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--tcp"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn cli_list_udp_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--udp"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn cli_completions_bash() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["completions", "bash"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "completions should produce output");
}

#[test]
fn list_exposed_filters_loopback() {
    let exposed = Helper::spawn("exposed");
    let local = Helper::spawn("tcp");
    let doc = json_of(&kav(&["list", "--json", "--exposed"]));
    assert!(
        !json_ports(&doc, exposed.port).is_empty(),
        "exposed helper listed"
    );
    assert!(
        json_ports(&doc, local.port).is_empty(),
        "loopback helper filtered out"
    );
    for row in doc["ports"].as_array().unwrap() {
        assert_eq!(row["exposed"], true, "{row}");
    }
}

#[test]
fn list_reverse_sorts_descending() {
    let _a = Helper::spawn("tcp");
    let _b = Helper::spawn("tcp");
    let doc = json_of(&kav(&["list", "--json", "--reverse"]));
    let ports: Vec<u64> = doc["ports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["port"].as_u64().unwrap())
        .collect();
    assert!(ports.len() >= 2);
    assert!(
        ports.windows(2).all(|w| w[0] >= w[1]),
        "ports not descending: {ports:?}"
    );
}

#[test]
fn watch_json_emits_ndjson() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--watch", "--json", "--interval", "0.3"])
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok).take(2) {
            let _ = tx.send(line);
        }
    });
    let mut frames = Vec::new();
    for _ in 0..2 {
        frames.push(
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .expect("watch frame"),
        );
    }
    let _ = child.kill();
    let _ = child.wait();
    for frame in frames {
        let v: serde_json::Value =
            serde_json::from_str(&frame).expect("frame is one JSON document");
        assert_eq!(v["schema"], 2);
        assert!(v["timestamp"].is_u64(), "frame has a timestamp: {frame}");
        assert!(v["ports"].is_array());
    }
}

#[test]
fn check_free_exit_codes() {
    let helper = Helper::spawn("tcp");
    assert_eq!(kav(&["check", "--free", "1"]).status.code(), Some(0));
    assert_eq!(
        kav(&["check", "--free", &helper.port.to_string()])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn check_quiet_prints_nothing() {
    let helper = Helper::spawn("tcp");
    let busy = kav(&["check", "-q", &helper.port.to_string()]);
    assert_eq!(busy.status.code(), Some(0));
    assert!(busy.stdout.is_empty(), "{}", stdout_of(&busy));
    let free = kav(&["check", "--quiet", "1"]);
    assert_eq!(free.status.code(), Some(1));
    assert!(free.stdout.is_empty());
}

#[test]
fn check_json_v2() {
    let helper = Helper::spawn("tcp");
    let out = kav(&["check", "--json", &helper.port.to_string(), "1"]);
    assert_eq!(out.status.code(), Some(0));
    let doc = json_of(&out);
    assert_eq!(doc["schema"], 2);
    let busy = &doc["ports"][0];
    assert_eq!(busy["port"], helper.port);
    assert_eq!(busy["in_use"], true);
    assert_eq!(busy["owner_visible"], true);
    assert_eq!(busy["listeners"][0]["pid"], helper.pid);
    assert!(busy["probe"].is_null());
    let free = &doc["ports"][1];
    assert_eq!(free["in_use"], false);
    assert_eq!(free["listeners"], serde_json::json!([]));
}

#[test]
fn check_dedupes_ports() {
    let helper = Helper::spawn("tcp");
    let p = helper.port.to_string();
    let out = kav(&["check", &p, &p, "1"]);
    assert!(
        stdout_of(&out).contains("1 of 2 ports active"),
        "{}",
        stdout_of(&out)
    );
}

#[test]
fn check_probe_reports_tcp_open() {
    let helper = Helper::spawn("tcp");
    let doc = json_of(&kav(&[
        "check",
        "--probe",
        "--json",
        &helper.port.to_string(),
    ]));
    assert_eq!(doc["ports"][0]["probe"]["result"], "tcp_open", "{doc}");
}

fn kav_kill(args: &[&str]) -> std::process::Output {
    let mut all = vec!["kill", "--yes"];
    all.extend_from_slice(args);
    kav(&all)
}

#[test]
fn kill_stops_helper_and_frees_port() {
    let mut helper = Helper::spawn("tcp");
    let out = kav_kill(&[&helper.port.to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout_of(&out));
    assert!(
        helper
            .wait_exit(std::time::Duration::from_secs(3))
            .is_some(),
        "helper exited"
    );
    assert!(stdout_of(&out).contains("Stopped"), "{}", stdout_of(&out));
}

#[test]
fn kill_reports_still_running_for_sigterm_ignorer() {
    let mut helper = Helper::spawn("noterm");
    let out = kav_kill(&["--timeout", "1", &helper.port.to_string()]);
    assert_eq!(out.status.code(), Some(3), "{}", stdout_of(&out));
    assert!(
        helper.is_running(),
        "SIGTERM is ignored, so the helper must still run"
    );
    let text = format!(
        "{}{}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("still running"), "{text}");
}

#[test]
fn kill_force_stops_sigterm_ignorer() {
    let mut helper = Helper::spawn("noterm");
    let out = kav_kill(&["--force", &helper.port.to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout_of(&out));
    assert!(helper
        .wait_exit(std::time::Duration::from_secs(3))
        .is_some());
}

#[test]
fn kill_escalate_stops_sigterm_ignorer() {
    let mut helper = Helper::spawn("noterm");
    let out = kav_kill(&["--timeout", "1", "--escalate", &helper.port.to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout_of(&out));
    assert!(helper
        .wait_exit(std::time::Duration::from_secs(3))
        .is_some());
}

#[test]
fn kill_nothing_listening_exits_1() {
    assert_eq!(kav_kill(&["1"]).status.code(), Some(1));
}

#[test]
fn kill_no_wait_returns_immediately() {
    let mut helper = Helper::spawn("noterm");
    let start = std::time::Instant::now();
    let out = kav_kill(&["--no-wait", &helper.port.to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout_of(&out));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "took {:?}",
        start.elapsed()
    );
    assert!(helper.is_running(), "--no-wait does not wait or escalate");
}

#[test]
fn kill_json_v2_outcome_exited() {
    let helper = Helper::spawn("tcp");
    let out = kav(&["kill", "--json", &helper.port.to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout_of(&out));
    let doc = json_of(&out);
    assert_eq!(doc["schema"], 2);
    let r = &doc["results"][0];
    assert_eq!(r["outcome"], "exited", "{doc}");
    assert_eq!(r["pid"], helper.pid);
    assert_eq!(r["action"], "signal");
    assert_eq!(r["signal"], "SIGTERM");
    assert_eq!(r["ports"], serde_json::json!([helper.port]));
}

#[test]
fn kill_detects_respawn() {
    let helper = Helper::spawn("supervisor");
    let out = kav_kill(&[&helper.port.to_string()]);
    assert_eq!(out.status.code(), Some(3), "{}", stdout_of(&out));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("taken again") && err.contains("--parent"),
        "{err}"
    );
}

#[test]
fn kill_parent_stops_supervisor_and_child() {
    let mut helper = Helper::spawn("supervisor");
    let port = helper.port.to_string();
    let out = kav_kill(&["--parent", &port]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        helper
            .wait_exit(std::time::Duration::from_secs(3))
            .is_some(),
        "supervisor stopped"
    );
    assert_eq!(
        kav(&["check", "-q", &port]).status.code(),
        Some(1),
        "port is free"
    );
}

#[test]
fn kill_parent_reports_orphaned_listener() {
    let helper = Helper::spawn("orphaner");
    let out = kav_kill(&["--parent", &helper.port.to_string()]);
    assert_eq!(out.status.code(), Some(3), "{}", stdout_of(&out));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("still held"), "{err}");
    assert!(
        !stdout_of(&out).contains("is free"),
        "must not claim the port is free"
    );
}
