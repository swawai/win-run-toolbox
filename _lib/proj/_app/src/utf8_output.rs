#[derive(Default)]
pub(crate) struct Utf8LossyDecoder {
    pending: Vec<u8>,
}

impl Utf8LossyDecoder {
    pub(crate) fn decode(&mut self, bytes: &[u8], eof: bool) -> Option<String> {
        let mut input = Vec::with_capacity(self.pending.len() + bytes.len());
        input.append(&mut self.pending);
        input.extend_from_slice(bytes);

        let mut text = String::new();
        let mut cursor = 0;
        while cursor < input.len() {
            match std::str::from_utf8(&input[cursor..]) {
                Ok(valid) => {
                    text.push_str(valid);
                    cursor = input.len();
                }
                Err(error) => {
                    let valid_end = cursor + error.valid_up_to();
                    text.push_str(
                        std::str::from_utf8(&input[cursor..valid_end])
                            .expect("from_utf8 reported this prefix as valid UTF-8"),
                    );
                    match error.error_len() {
                        Some(length) => {
                            text.push('\u{fffd}');
                            cursor = valid_end + length;
                        }
                        None => {
                            self.pending.extend_from_slice(&input[valid_end..]);
                            cursor = input.len();
                        }
                    }
                }
            }
        }

        if eof && !self.pending.is_empty() {
            text.push_str(&String::from_utf8_lossy(&self.pending));
            self.pending.clear();
        }
        (!text.is_empty()).then_some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_utf8_split_across_read_boundaries() {
        let mut decoder = Utf8LossyDecoder::default();
        let bytes = "甲".as_bytes();

        assert_eq!(decoder.decode(&bytes[..2], false), None);
        assert_eq!(decoder.decode(&bytes[2..], false), Some("甲".to_owned()));
        assert_eq!(decoder.decode(&[], true), None);
    }

    #[test]
    fn makes_invalid_and_incomplete_bytes_visible() {
        let mut decoder = Utf8LossyDecoder::default();

        assert_eq!(
            decoder.decode(&[b'a', 0xff, 0xe7], false),
            Some("a�".to_owned())
        );
        assert_eq!(decoder.decode(&[], true), Some("�".to_owned()));
    }
}
