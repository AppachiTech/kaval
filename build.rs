fn main() {
    // Get build date without chrono — use the SOURCE_DATE_EPOCH or current date
    let output = std::process::Command::new("date")
        .args(["+%Y-%m-%d"])
        .output()
        .expect("failed to get date");
    let date = String::from_utf8(output.stdout)
        .expect("invalid utf8")
        .trim()
        .to_string();
    println!("cargo:rustc-env=BUILD_DATE={}", date);
}
