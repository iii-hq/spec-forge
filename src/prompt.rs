use crate::types::Catalog;

pub fn build_prompt(user_prompt: &str, catalog: &Catalog) -> String {
    let mut prompt = String::with_capacity(8192);

    prompt.push_str(r#"You are an expert UI architect. Generate a structured JSON UI spec using ONLY the provided components.

## Design Principles
- Use semantic hierarchy: wrap related elements in container components (Card, Stack, Grid)
- Keep the element tree shallow — prefer 2-3 levels of nesting max
- Use meaningful, short element IDs like "header", "form-card", "submit-btn"
- Prefer layout components (Stack, Grid) to organize groups of elements
- Every form should have labeled inputs, clear grouping, and a primary action button
- Use props exactly as defined — do not invent props that don't exist
- Text content should be realistic and specific, not placeholder lorem ipsum

"#);

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

Return ONLY valid JSON (no markdown, no backticks, no explanation) in this exact format:
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

## Rules
- Use ONLY components listed above — never invent new component types
- Every element ID must be unique, short, and descriptive (e.g. "main-card", "email-input", "submit-btn")
- Props must match the component's defined props exactly — no extra props
- Children array contains element IDs (strings), not inline objects
- Root must reference an existing element ID
- Leaf elements must have "children": []
- Use containers (Card, Stack, Grid) to group related elements logically
- Generate 5-12 elements for a complete, well-structured UI
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

    #[test]
    fn prompt_includes_design_principles() {
        let catalog = Catalog {
            components: BTreeMap::new(),
            actions: BTreeMap::new(),
        };
        let result = build_prompt("test", &catalog);
        assert!(result.contains("Design Principles"));
        assert!(result.contains("semantic hierarchy"));
    }
}
