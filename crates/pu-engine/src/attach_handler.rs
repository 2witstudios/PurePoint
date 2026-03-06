const APC_START: &[u8] = b"\x1b_RESIZE:";
const APC_END: u8 = b'\\';
const ESC: u8 = 0x1b;

/// Parse "cols:rows" from a byte slice.
fn parse_dims(payload: &[u8]) -> Option<(u16, u16)> {
    let s = std::str::from_utf8(payload).ok()?;
    let mut parts = s.split(':');
    let cols: u16 = parts.next()?.parse().ok()?;
    let rows: u16 = parts.next()?.parse().ok()?;
    Some((cols, rows))
}

/// Find the APC payload and ST terminator position within input that starts with APC_START.
/// Returns (payload_before_ST, position_after_ST_in_payload) or None.
fn find_apc_payload(input: &[u8]) -> Option<(&[u8], usize)> {
    if !input.starts_with(APC_START) {
        return None;
    }
    let payload = &input[APC_START.len()..];
    let end_pos = payload
        .windows(2)
        .position(|w| w[0] == ESC && w[1] == APC_END)?;
    Some((&payload[..end_pos], APC_START.len() + end_pos + 2))
}

/// Parse an APC resize escape: `\x1b_RESIZE:{cols}:{rows}\x1b\`
pub fn parse_apc_resize(input: &[u8]) -> Option<(u16, u16)> {
    let (dims_bytes, _) = find_apc_payload(input)?;
    parse_dims(dims_bytes)
}

/// Strip an APC resize escape from the front of input, returning the resize and remaining bytes.
pub fn strip_apc_resize(input: &[u8]) -> (Option<(u16, u16)>, &[u8]) {
    match find_apc_payload(input) {
        Some((dims_bytes, consumed)) => (parse_dims(dims_bytes), &input[consumed..]),
        None => (None, input),
    }
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
