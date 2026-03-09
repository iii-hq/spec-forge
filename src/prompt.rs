use crate::types::Catalog;

pub fn build_prompt(user_prompt: &str, catalog: &Catalog) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str(
        "You are a UI generator. Generate a JSON spec using ONLY these components:\n\n",
    );

    prompt.push_str("## Available Components\n\n");
    for (name, def) in &catalog.components {
        prompt.push_str(&format!("### {}\n", name));
        prompt.push_str(&format!("Description: {}\n", def.description));
        if !def.props.is_null() {
            prompt.push_str(&format!(
                "Props: {}\n",
                serde_json::to_string(&def.props).unwrap_or_default()
            ));
        }
        if def.children {
            prompt.push_str("Can have children: yes\n");
        }
        prompt.push('\n');
    }

    if !catalog.actions.is_empty() {
        prompt.push_str("## Available Actions\n\n");
        for (name, def) in &catalog.actions {
            prompt.push_str(&format!("- **{}**: {}\n", name, def.description));
        }
        prompt.push('\n');
    }

    prompt.push_str(
        r#"## Output Format

Return ONLY valid JSON in this exact format:
{
  "root": "<root-element-id>",
  "elements": {
    "<element-id>": {
      "type": "<ComponentName>",
      "props": { ... },
      "children": ["<child-id>", ...]
    }
  }
}

Rules:
- Use ONLY components listed above
- Every element ID must be unique (format: lowercase-type-number, e.g. "card-1")
- Props must match the component's props schema exactly
- Children array contains element IDs, not inline elements
- Root must reference an existing element ID
"#,
    );

    prompt.push_str("\n## User Request\n\n");
    prompt.push_str(user_prompt);
    prompt.push_str("\n\nGenerate the JSON spec now:");

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
        assert!(result.contains("Can have children: yes"));
        assert!(result.contains("Show a dashboard"));
        assert!(result.contains("root"));
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
        assert!(!result.contains("Available Actions"));
    }
}
