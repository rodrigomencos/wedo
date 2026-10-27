//! Copying text to the system clipboard.

use std::io::{self, Write};
use std::process::{Command, Stdio};

const TOOLS: &[(&str, &[&str])] = &[
    ("pbcopy", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
    ("clip.exe", &[]),
];

/// Copies `text` and says where it went. Uses the platform's clipboard tool when one
/// works, otherwise OSC 52, which asks the terminal to do it. Over SSH, OSC 52 goes
/// first: the remote machine's clipboard isn't the one in front of you.
pub(super) fn copy(text: &str) -> io::Result<&'static str> {
    let remote = std::env::var_os("SSH_TTY").is_some();
    if !remote
        && TOOLS
            .iter()
            .any(|&(program, args)| pipe_to(program, args, text).is_ok())
    {
        return Ok("clipboard");
    }
    let mut out = io::stdout();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    out.flush()?;
    Ok("terminal clipboard")
}

fn pipe_to(program: &str, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Dropping stdin after the write closes it, so the tool sees the end of the text.
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(text.as_bytes())?;
    if child.wait()?.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{program} failed")))
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits =
            chunk.iter().fold(0u32, |n, &b| n << 8 | u32::from(b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            let sextet = (bits >> (18 - 6 * i)) & 63;
            out.push(if i <= chunk.len() {
                ALPHABET[sextet as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn encodes_base64_with_padding() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, expected) in cases {
            assert_eq!(base64(input.as_bytes()), expected);
        }
        assert_eq!(base64("日本".as_bytes()), "5pel5pys");
    }
}
