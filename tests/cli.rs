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

/// Extract `"cpu":<value>` from the v1 JSON line describing `port`.
fn json_cpu_for_port(json: &str, port: u16) -> Option<f64> {
    let needle = format!("\"port\":{port},");
    let line = json.lines().find(|l| l.contains(&needle))?;
    let rest = line.split("\"cpu\":").nth(1)?;
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    num.parse().ok()
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
    std::thread::sleep(std::time::Duration::from_millis(300));
    let out = kav(&["list", "--json"]);
    let cpu = json_cpu_for_port(&stdout_of(&out), helper.port)
        .unwrap_or_else(|| panic!("busy helper missing from JSON:\n{}", stdout_of(&out)));
    assert!(
        cpu >= 30.0,
        "busy helper spins a full core but kav reported {cpu}% CPU"
    );
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
#[ignore = "red until Task 4b keys rows by protocol"]
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with('['));
    assert!(stdout.trim_end().ends_with(']'));
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse every "port":<N> value and assert it is exactly 80
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("{\"port\":") {
            // rest starts with the port number, e.g. "80,\"protocol\":..."
            let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            let port: u16 = port_str
                .parse()
                .unwrap_or_else(|_| panic!("failed to parse port from: {}", line));
            assert_eq!(port, 80, "filter '80' matched non-80 port in: {}", line);
        }
    }
}

#[test]
fn cli_list_filter_json_combo() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--json", "--filter", "zzzunlikelyservice"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // No match → JSON empty array
    assert_eq!(stdout.trim(), "[]");
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
fn cli_list_watch_json_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_kav"))
        .args(["list", "--watch", "--json"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "--watch and --json must be mutually exclusive"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mutually exclusive"),
        "error message should mention mutual exclusion"
    );
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
