const APC_START: &[u8] = b"\x1b_RESIZE:";
const APC_END: u8 = b'\\';
const ESC: u8 = 0x1b;

/// Parse an APC resize escape: `\x1b_RESIZE:{cols}:{rows}\x1b\`
pub fn parse_apc_resize(input: &[u8]) -> Option<(u16, u16)> {
    if !input.starts_with(APC_START) {
        return None;
    }
    // Find the ST (string terminator): ESC + backslash
    let payload = &input[APC_START.len()..];
    let end_pos = payload
        .windows(2)
        .position(|w| w[0] == ESC && w[1] == APC_END)?;
    let dims = &payload[..end_pos];
    let s = std::str::from_utf8(dims).ok()?;
    let mut parts = s.split(':');
    let cols: u16 = parts.next()?.parse().ok()?;
    let rows: u16 = parts.next()?.parse().ok()?;
    Some((cols, rows))
}

/// Strip an APC resize escape from the front of input, returning the resize and remaining bytes.
pub fn strip_apc_resize(input: &[u8]) -> (Option<(u16, u16)>, &[u8]) {
    if !input.starts_with(APC_START) {
        return (None, input);
    }
    let payload = &input[APC_START.len()..];
    if let Some(end_pos) = payload
        .windows(2)
        .position(|w| w[0] == ESC && w[1] == APC_END)
    {
        let rest = &payload[end_pos + 2..];
        return (parse_apc_resize(input), rest);
    }
    (None, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_apc_resize_escape_should_parse_cols_and_rows() {
        let input = b"\x1b_RESIZE:120:40\x1b\\rest";
        let result = parse_apc_resize(input);
        assert_eq!(result, Some((120, 40)));
    }

    #[test]
    fn given_no_apc_escape_should_return_none() {
        let input = b"normal input text";
        let result = parse_apc_resize(input);
        assert!(result.is_none());
    }

    #[test]
    fn given_partial_apc_escape_should_return_none() {
        let input = b"\x1b_RESIZE:120";
        let result = parse_apc_resize(input);
        assert!(result.is_none());
    }

    #[test]
    fn given_strip_apc_should_return_remaining_bytes() {
        let input = b"\x1b_RESIZE:80:24\x1b\\hello";
        let (resize, rest) = strip_apc_resize(input);
        assert_eq!(resize, Some((80, 24)));
        assert_eq!(rest, b"hello");
    }

    #[test]
    fn given_no_apc_strip_should_return_full_input() {
        let input = b"plain text";
        let (resize, rest) = strip_apc_resize(input);
        assert!(resize.is_none());
        assert_eq!(rest, input);
    }
}
