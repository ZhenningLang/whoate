use serde_json::Value;

/// Replace comments with spaces so tokens cannot join; leave strings untouched.
/// Both passes advance monotonically and allocate at most the input length.
pub(super) fn parse(text: &str) -> Result<Value, &'static str> {
    let mut bytes = text.as_bytes().to_vec();
    let mut i = 0;
    let mut string = false;
    while i < bytes.len() {
        if string {
            match bytes[i] {
                b'\\' => {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                b'"' => string = false,
                _ => {}
            }
        } else {
            match (bytes[i], bytes.get(i + 1).copied()) {
                (b'"', _) => string = true,
                (b'/', Some(b'/')) => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        bytes[i] = b' ';
                        i += 1;
                    }
                    continue;
                }
                (b'/', Some(b'*')) => {
                    bytes[i] = b' ';
                    bytes[i + 1] = b' ';
                    i += 2;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        if bytes[i] != b'\n' && bytes[i] != b'\r' {
                            bytes[i] = b' ';
                        }
                        i += 1;
                    }
                    if i + 1 >= bytes.len() {
                        return Err("invalid_jsonc");
                    }
                    bytes[i] = b' ';
                    bytes[i + 1] = b' ';
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        i += 1;
    }
    i = 0;
    string = false;
    let mut previous = None;
    while i < bytes.len() {
        if string {
            match bytes[i] {
                b'\\' => {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                b'"' => string = false,
                _ => {}
            }
        } else if bytes[i] == b'"' {
            string = true;
        } else if bytes[i] == b',' && !matches!(previous, None | Some(b'{' | b'[' | b',' | b':')) {
            let mut next = i + 1;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if matches!(bytes.get(next), Some(b'}' | b']')) {
                bytes[i] = b' ';
            }
        }
        if !bytes[i].is_ascii_whitespace() {
            previous = Some(bytes[i]);
        }
        i += 1;
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid_jsonc")
}
