use std::io::Write;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// copy hands text to the terminal's clipboard as OSC52. It goes to /dev/tty,
/// the same terminal the finder draws on: stdout belongs to the shell binding
/// that captures gm's output.
pub fn copy(text: &str) -> std::io::Result<()> {
    let mut tty = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .map_err(|e| std::io::Error::other(format!("no terminal to copy to: {e}")))?;
    write!(tty, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    tty.flush()
}

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = |i: usize| chunk.get(i).copied().unwrap_or(0);
        let quad = ((b(0) as u32) << 16) | ((b(1) as u32) << 8) | b(2) as u32;
        out.push(ALPHABET[(quad >> 18 & 0b111_111) as usize] as char);
        out.push(ALPHABET[(quad >> 12 & 0b111_111) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(quad >> 6 & 0b111_111) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(quad & 0b111_111) as usize] as char);
        }
    }
    out.push_str(&"=".repeat((4 - out.len() % 4) % 4));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encodes_rfc4648() {
        for (s, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
            (
                "https://github.com/acme/alpha/issues/7",
                "aHR0cHM6Ly9naXRodWIuY29tL2FjbWUvYWxwaGEvaXNzdWVzLzc=",
            ),
        ] {
            assert_eq!(base64(s.as_bytes()), want, "{s:?}");
        }
    }
}
