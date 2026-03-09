use crate::types::{UIElement, UISpec};
use std::collections::{HashMap, HashSet, VecDeque};

pub struct IncrementalJsonParser {
    buffer: String,
    extracted: VecDeque<(String, UIElement)>,
    emitted_keys: HashSet<String>,
    all_elements: HashMap<String, UIElement>,
    root: Option<String>,
}

impl IncrementalJsonParser {
    pub fn new() -> Self {
        Self {
            buffer: String::with_capacity(16384),
            extracted: VecDeque::new(),
            emitted_keys: HashSet::new(),
            all_elements: HashMap::new(),
            root: None,
        }
    }

    pub fn feed(&mut self, chunk: &str) {
        self.buffer.push_str(chunk);
        self.try_extract();
    }

    pub fn next_element(&mut self) -> Option<(String, UIElement)> {
        self.extracted.pop_front()
    }

    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    pub fn finalize(&self) -> Option<UISpec> {
        let root = self.root.clone()?;
        if self.all_elements.is_empty() {
            return None;
        }
        Some(UISpec {
            root,
            elements: self.all_elements.clone(),
        })
    }

    fn try_extract(&mut self) {
        if self.root.is_none() {
            if let Some(pos) = self.buffer.find("\"root\"") {
                let after = &self.buffer[pos..];
                if let Some(colon) = after.find(':') {
                    let after_colon = after[colon + 1..].trim_start();
                    if after_colon.starts_with('"') {
                        if let Some(end_quote) = after_colon[1..].find('"') {
                            self.root = Some(after_colon[1..1 + end_quote].to_string());
                        }
                    }
                }
            }
        }

        if let Some(elements_pos) = self.buffer.find("\"elements\"") {
            let buf_clone = self.buffer.clone();
            let after = &buf_clone[elements_pos..];
            if let Some(brace_start) = after.find('{') {
                let elements_content = &after[brace_start + 1..];
                self.extract_elements_from(elements_content);
            }
        }
    }

    fn extract_elements_from(&mut self, content: &str) {
        let mut pos = 0;
        let bytes = content.as_bytes();

        while pos < bytes.len() {
            while pos < bytes.len() && bytes[pos] != b'"' {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }

            pos += 1;
            let key_start = pos;
            while pos < bytes.len() && bytes[pos] != b'"' {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }
            let key = &content[key_start..pos];
            pos += 1;

            if key == "elements" || key == "root" {
                continue;
            }

            while pos < bytes.len() && bytes[pos] != b'{' {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }

            let obj_start = pos;
            let mut depth = 0;
            let mut in_string = false;
            let mut escape_next = false;
            let mut found_end = false;

            for i in pos..bytes.len() {
                if escape_next {
                    escape_next = false;
                    continue;
                }
                match bytes[i] {
                    b'\\' if in_string => escape_next = true,
                    b'"' => in_string = !in_string,
                    b'{' if !in_string => depth += 1,
                    b'}' if !in_string => {
                        depth -= 1;
                        if depth == 0 {
                            let obj_str = &content[obj_start..=i];
                            if let Ok(element) = serde_json::from_str::<UIElement>(obj_str) {
                                if self.emitted_keys.insert(key.to_string()) {
                                    self.all_elements.insert(key.to_string(), element.clone());
                                    self.extracted.push_back((key.to_string(), element));
                                } else {
                                    self.all_elements.insert(key.to_string(), element);
                                }
                            }
                            pos = i + 1;
                            found_end = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if !found_end {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_complete_spec() {
        let mut parser = IncrementalJsonParser::new();
        parser.feed(r#"{"root":"card-1","elements":{"card-1":{"type":"Card","props":{"title":"Hello"},"children":["btn-1"]},"btn-1":{"type":"Button","props":{"label":"Click"},"children":[]}}}"#);

        assert_eq!(parser.root(), Some("card-1"));

        let mut elements = Vec::new();
        while let Some(el) = parser.next_element() {
            elements.push(el);
        }
        assert_eq!(elements.len(), 2);

        let spec = parser.finalize().unwrap();
        assert_eq!(spec.root, "card-1");
        assert_eq!(spec.elements.len(), 2);
    }

    #[test]
    fn parse_streaming_chunks() {
        let mut parser = IncrementalJsonParser::new();

        parser.feed(r#"{"root":"card-1","elements":{"card-1":{"type":"Card","props":{"title":"Hello"},"children":[]}"#);
        assert_eq!(parser.root(), Some("card-1"));
        assert!(parser.next_element().is_some()); // card-1

        parser.feed(r#","btn-1":{"type":"Button","props":{"label":"Go"},"children":[]}}}"#);
        assert!(parser.next_element().is_some()); // btn-1

        let spec = parser.finalize().unwrap();
        assert_eq!(spec.elements.len(), 2);
    }

    #[test]
    fn parse_root_extraction() {
        let mut parser = IncrementalJsonParser::new();
        parser.feed(r#"{"root": "dashboard-1", "elements": {}}"#);
        assert_eq!(parser.root(), Some("dashboard-1"));
    }

    #[test]
    fn incomplete_element_waits() {
        let mut parser = IncrementalJsonParser::new();
        parser.feed(r#"{"root":"x","elements":{"card-1":{"type":"Card","props":{"title":"He"#);
        assert!(parser.next_element().is_none());

        parser.feed(r#"llo"},"children":[]}}}"#);
        assert!(parser.next_element().is_some());
    }

    #[test]
    fn no_duplicate_emissions() {
        let mut parser = IncrementalJsonParser::new();
        parser.feed(r#"{"root":"a","elements":{"a":{"type":"X","props":{},"children":[]}"#);
        assert!(parser.next_element().is_some()); // a emitted

        parser.feed(r#"}}"#);
        assert!(parser.next_element().is_none()); // a NOT re-emitted
    }

    #[test]
    fn finalize_builds_from_accumulated() {
        let mut parser = IncrementalJsonParser::new();
        parser.feed(r#"{"root":"card-1","elements":{"card-1":{"type":"Card","props":{},"children":["b-1"]}"#);
        while parser.next_element().is_some() {}

        parser.feed(r#","b-1":{"type":"Btn","props":{},"children":[]}}}"#);
        while parser.next_element().is_some() {}

        let spec = parser.finalize().unwrap();
        assert_eq!(spec.root, "card-1");
        assert_eq!(spec.elements.len(), 2);
        assert!(spec.elements.contains_key("card-1"));
        assert!(spec.elements.contains_key("b-1"));
    }
}
