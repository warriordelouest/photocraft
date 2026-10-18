//! Server-sent events, as both formats stream them: `event:` and `data:` lines, an empty line
//! ends an event. Fed line by line so it works on any reader.

#[derive(Default)]
pub struct Parser {
    event: String,
    data: String,
}

impl Parser {
    /// Feeds one line (without its line ending). Returns `(event, data)` when the line completes
    /// an event that carries data.
    pub fn line(&mut self, line: &str) -> Option<(String, String)> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            let event = std::mem::take(&mut self.event);
            let data = std::mem::take(&mut self.data);
            return (!data.is_empty()).then_some((event, data));
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value);
            }
            _ => {}
        }
        None
    }

    /// The event left when the stream ends without a final empty line.
    pub fn finish(&mut self) -> Option<(String, String)> {
        self.line("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(text: &str) -> Vec<(String, String)> {
        let mut p = Parser::default();
        let mut out: Vec<_> = text.split('\n').filter_map(|l| p.line(l)).collect();
        out.extend(p.finish());
        out
    }

    #[test]
    fn events_data_comments_and_crlf() {
        let got = feed("event: a\r\ndata: {\"x\":1}\r\n\r\n: keep-alive\n\ndata: [DONE]\n\n");
        assert_eq!(got, vec![("a".into(), "{\"x\":1}".into()), ("".into(), "[DONE]".into())]);
    }

    #[test]
    fn multi_line_data_joins_with_newlines_and_a_trailing_event_is_kept() {
        let got = feed("data: one\ndata:two\nevent: e");
        assert_eq!(got, vec![("e".into(), "one\ntwo".into())]);
    }

    #[test]
    fn events_without_data_are_dropped() {
        assert!(feed("event: ping\n\n\n").is_empty());
    }
}
