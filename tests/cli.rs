use std::process::Command;

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
