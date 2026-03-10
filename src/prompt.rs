use crate::types::Catalog;

pub fn build_prompt(user_prompt: &str, catalog: &Catalog) -> String {
    let mut prompt = String::with_capacity(8192);

    prompt.push_str(r#"You are a UI generator that outputs JSONL (one JSON object per line).

OUTPUT FORMAT (JSONL, RFC 6902 JSON Patch):
Output one JSON patch operation per line to build a UI spec progressively.
Each line MUST be a complete, valid JSON object. No markdown, no code fences, no explanation.

Start with the root, then add elements one at a time so the UI fills in progressively.

Example output (each line is a separate JSON object):

{"op":"add","path":"/root","value":"main"}
{"op":"add","path":"/elements/main","value":{"type":"Card","props":{"title":"Dashboard"},"children":["metric-1","chart"]}}
{"op":"add","path":"/elements/metric-1","value":{"type":"Metric","props":{"label":"Revenue","value":"$42K"},"children":[]}}
{"op":"add","path":"/elements/chart","value":{"type":"Card","props":{"title":"Sales"},"children":[]}}

"#);

    prompt.push_str("AVAILABLE COMPONENTS:\n");
    for (name, def) in &catalog.components {
        let mut line = format!("- {}", name);
        if !def.props.is_null() {
            line.push_str(&format!(
                ": {}",
                serde_json::to_string(&def.props).unwrap_or_default()
            ));
        }
        line.push_str(&format!(" - {}", def.description));
        if def.children {
            line.push_str(" [accepts children]");
        }
        prompt.push_str(&line);
        prompt.push('\n');
    }

    if !catalog.actions.is_empty() {
        prompt.push_str("\nAVAILABLE ACTIONS:\n");
        for (name, def) in &catalog.actions {
            prompt.push_str(&format!("- {}: {}\n", name, def.description));
        }
    }

    prompt.push_str(r#"
RULES:
1. Output ONLY JSONL patches — one JSON object per line, no markdown, no code fences, no text
2. First line MUST set root: {"op":"add","path":"/root","value":"<root-key>"}
3. Then add each element: {"op":"add","path":"/elements/<key>","value":{"type":"...","props":{...},"children":[...]}}
4. Use ONLY components listed above — never invent new types
5. Each element needs: type, props, children (array of child keys, empty [] for leaves)
6. Use unique, descriptive keys (e.g. "form-card", "email-input", "submit-btn")
7. Use layout components (Stack, Grid, Card) to group related elements
8. Props must match the component's defined props exactly
9. Generate 5-12 elements for a complete, well-structured UI
10. Text content should be realistic and specific
"#);

    prompt.push_str("\nUSER REQUEST: ");
    prompt.push_str(user_prompt);
    prompt.push_str("\n\nOutput JSONL patches now:\n");

    prompt
}

pub fn build_refine_prompt(
    user_prompt: &str,
    current_spec: &crate::types::UISpec,
    catalog: &Catalog,
) -> String {
    let mut prompt = String::with_capacity(8192);

    prompt.push_str(r#"You are a UI editor. Given an existing UI spec and a change request, output JSONL patches (RFC 6902) to modify the spec.

OPERATIONS:
- Add new element: {"op":"add","path":"/elements/<key>","value":{"type":"...","props":{...},"children":[...]}}
- Replace element: {"op":"replace","path":"/elements/<key>","value":{"type":"...","props":{...},"children":[...]}}
- Remove element: {"op":"remove","path":"/elements/<key>"}
- Change root: {"op":"replace","path":"/root","value":"<new-root-key>"}

Output ONLY JSONL patches — one JSON object per line. No text, no markdown.

"#);

    prompt.push_str("CURRENT SPEC:\n");
    prompt.push_str(&serde_json::to_string_pretty(current_spec).unwrap_or_default());
    prompt.push('\n');

    prompt.push_str("\nAVAILABLE COMPONENTS:\n");
    for (name, def) in &catalog.components {
        let mut line = format!("- {}", name);
        if !def.props.is_null() {
            line.push_str(&format!(
                ": {}",
                serde_json::to_string(&def.props).unwrap_or_default()
            ));
        }
        line.push_str(&format!(" - {}", def.description));
        if def.children {
            line.push_str(" [accepts children]");
        }
        prompt.push_str(&line);
        prompt.push('\n');
    }

    prompt.push_str("\nCHANGE REQUEST: ");
    prompt.push_str(user_prompt);
    prompt.push_str("\n\nOutput JSONL patches now:\n");

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::collections::BTreeMap;

    #[test]
    fn prompt_includes_components() {
        let mut components = BTreeMap::new();
        components.insert(
            "Card".into(),
            ComponentDef {
                description: "A card container".into(),
                props: serde_json::json!({"title": "string"}),
                children: true,
            },
        );
        let catalog = Catalog {
            components,
            actions: BTreeMap::new(),
        };

        let result = build_prompt("Show a dashboard", &catalog);
        assert!(result.contains("Card"));
        assert!(result.contains("A card container"));
        assert!(result.contains("[accepts children]"));
        assert!(result.contains("Show a dashboard"));
        assert!(result.contains("/root"));
    }

    #[test]
    fn prompt_includes_actions() {
        let mut actions = BTreeMap::new();
        actions.insert(
            "export".into(),
            ActionDef {
                description: "Export to PDF".into(),
            },
        );
        let catalog = Catalog {
            components: BTreeMap::new(),
            actions,
        };

        let result = build_prompt("test", &catalog);
        assert!(result.contains("export"));
        assert!(result.contains("Export to PDF"));
    }

    #[test]
    fn prompt_skips_actions_section_when_empty() {
        let catalog = Catalog {
            components: BTreeMap::new(),
            actions: BTreeMap::new(),
        };
        let result = build_prompt("test", &catalog);
        assert!(!result.contains("AVAILABLE ACTIONS"));
    }

    #[test]
    fn prompt_uses_jsonl_format() {
        let catalog = Catalog {
            components: BTreeMap::new(),
            actions: BTreeMap::new(),
        };
        let result = build_prompt("test", &catalog);
        assert!(result.contains("JSONL"));
        assert!(result.contains("RFC 6902"));
        assert!(result.contains(r#""op":"add""#));
    }

    #[test]
    fn refine_prompt_includes_current_spec() {
        let spec = UISpec {
            root: "main".into(),
            elements: Default::default(),
        };
        let catalog = Catalog {
            components: BTreeMap::new(),
            actions: BTreeMap::new(),
        };
        let result = build_refine_prompt("add a button", &spec, &catalog);
        assert!(result.contains("CURRENT SPEC"));
        assert!(result.contains("add a button"));
        assert!(result.contains(r#""op":"replace""#));
    }
}
