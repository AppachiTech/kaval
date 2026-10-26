//! Clipboard and "open in browser" helpers for the TUI.

use std::io::Write;
use std::process::{Command, Stdio};

/// Copy `text` to the clipboard. Returns how it was copied.
///
/// Tries the platform tools first (pbcopy, wl-copy, xclip, xsel), then falls
/// back to OSC 52, which asks the terminal itself — this also works over SSH
/// and inside tmux (with `set-clipboard on`).
pub fn copy(text: &str) -> Result<&'static str, String> {
    for (prog, args) in candidates() {
        if pipe_to(prog, args, text) {
            return Ok(prog);
        }
    }
    osc52(text)
        .map(|()| "terminal (OSC 52)")
        .map_err(|e| format!("no clipboard available: {e}"))
}

fn candidates() -> Vec<(&'static str, &'static [&'static str])> {
    let mut list: Vec<(&'static str, &'static [&'static str])> = Vec::new();
    if cfg!(target_os = "macos") {
        list.push(("pbcopy", &[]));
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        list.push(("wl-copy", &[]));
    }
    if std::env::var_os("DISPLAY").is_some() {
        list.push(("xclip", &["-selection", "clipboard"]));
        list.push(("xsel", &["--clipboard", "--input"]));
    }
    list
}

/// Feed `text` to a clipboard tool. Its output is discarded so error
/// messages cannot garble the TUI.
fn pipe_to(prog: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    let ok = child.wait().is_ok_and(|status| status.success());
    written && ok
}

/// The OSC 52 escape that sets the system clipboard to `text`.
pub fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

fn osc52(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    out.write_all(osc52_sequence(text).as_bytes())?;
    out.flush()
}

/// Standard base64 (RFC 4648) with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Open `url` with the system handler without blocking the UI. The child is
/// reaped on a thread so it never lingers as a zombie.
pub fn open_url(url: &str) -> bool {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match Command::new(opener)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
            ("3000", "MzAwMA=="),
        ];
        for (input, want) in cases {
            assert_eq!(base64(input.as_bytes()), want, "{input:?}");
        }
    }

    #[test]
    fn osc52_wraps_base64_payload() {
        assert_eq!(osc52_sequence("hi"), "\x1b]52;c;aGk=\x07");
    }
}
