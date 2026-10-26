fn main() {
    // Release builds pin the date with SOURCE_DATE_EPOCH so the same commit always
    // produces the same binary (https://reproducible-builds.org/specs/source-date-epoch/).
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    // Declaring any rerun-if line drops Cargo's "rerun on any change" default;
    // keep refreshing the date whenever the sources change.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");

    let secs = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => value
            .trim()
            .parse::<u64>()
            .expect("SOURCE_DATE_EPOCH must be a Unix timestamp in seconds"),
        // Cross-platform build date — no external commands
        Err(_) => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs(),
    };

    // Convert epoch seconds to YYYY-MM-DD
    let days = (secs / 86400) as i64;
    let (year, month, day) = days_to_ymd(days);
    let date = format!("{:04}-{:02}-{:02}", year, month, day);

    println!("cargo:rustc-env=BUILD_DATE={}", date);
}

/// Convert days since Unix epoch to (year, month, day)
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Civil calendar algorithm from Howard Hinnant
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
