use crate::types::{Catalog, UIElement, UISpec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum SpecPatch {
    Add {
        id: String,
        element: UIElement,
    },
    Replace {
        id: String,
        element: UIElement,
    },
    Remove {
        id: String,
    },
    #[serde(rename = "set_root")]
    SetRoot {
        root: String,
    },
}

pub fn diff_specs(old: &UISpec, new_spec: &UISpec) -> Vec<SpecPatch> {
    let mut patches = Vec::new();

    if old.root != new_spec.root {
        patches.push(SpecPatch::SetRoot {
            root: new_spec.root.clone(),
        });
    }

    for (id, new_el) in &new_spec.elements {
        match old.elements.get(id) {
            None => patches.push(SpecPatch::Add {
                id: id.clone(),
                element: new_el.clone(),
            }),
            Some(old_el) => {
                if !elements_equal(old_el, new_el) {
                    patches.push(SpecPatch::Replace {
                        id: id.clone(),
                        element: new_el.clone(),
                    });
                }
            }
        }
    }

    for id in old.elements.keys() {
        if !new_spec.elements.contains_key(id) {
            patches.push(SpecPatch::Remove { id: id.clone() });
        }
    }

    patches
}

pub fn apply_patches(base: &UISpec, patches: &[SpecPatch]) -> UISpec {
    let mut result = base.clone();

    for patch in patches {
        match patch {
            SpecPatch::Add { id, element } | SpecPatch::Replace { id, element } => {
                result.elements.insert(id.clone(), element.clone());
            }
            SpecPatch::Remove { id } => {
                result.elements.remove(id);
                for el in result.elements.values_mut() {
                    el.children.retain(|c| c != id);
                }
            }
            SpecPatch::SetRoot { root } => {
                result.root = root.clone();
            }
        }
    }

    result
}

fn elements_equal(a: &UIElement, b: &UIElement) -> bool {
    a.element_type == b.element_type && a.props == b.props && a.children == b.children
}

pub fn build_diff_prompt(user_prompt: &str, current_spec: &UISpec, catalog: &Catalog) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str("You are a UI updater. You have an EXISTING UI spec. The user wants a change.\n");
    prompt.push_str("Output ONLY the JSON patches needed — do NOT regenerate the full spec.\n\n");

    prompt.push_str("## Current Spec\n\n```json\n");
    prompt.push_str(&serde_json::to_string_pretty(current_spec).unwrap_or_default());
    prompt.push_str("\n```\n\n");

    prompt.push_str("## Available Components\n\n");
    for (name, def) in &catalog.components {
        prompt.push_str(&format!("- **{}**: {}\n", name, def.description));
    }
    prompt.push('\n');

    prompt.push_str("## Output Format\n\n");
    prompt.push_str(r#"Return a JSON array of patches:
[
  {"op": "add", "id": "<new-id>", "element": {"type": "...", "props": {...}, "children": [...]}},
  {"op": "replace", "id": "<existing-id>", "element": {"type": "...", "props": {...}, "children": [...]}},
  {"op": "remove", "id": "<id-to-remove>"},
  {"op": "set_root", "root": "<new-root-id>"}
]

Rules:
- ONLY output patches for elements that need to change
- Do NOT include unchanged elements
- When adding children to a parent, output a "replace" for the parent with updated children array
- Use existing element IDs when replacing
"#);

    prompt.push_str("\n## User Request\n\n");
    prompt.push_str(user_prompt);
    prompt.push_str("\n\nOutput the patches JSON now:");

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(t: &str, props: serde_json::Value, children: Vec<&str>) -> UIElement {
        UIElement {
            element_type: t.into(),
            props,
            children: children.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    fn make_spec(root: &str, elements: Vec<(&str, UIElement)>) -> UISpec {
        UISpec {
            root: root.into(),
            elements: elements
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    #[test]
    fn no_changes() {
        let spec = make_spec("a", vec![("a", el("Card", serde_json::json!({}), vec![]))]);
        let patches = diff_specs(&spec, &spec);
        assert!(patches.is_empty());
    }

    #[test]
    fn add_element() {
        let old = make_spec("a", vec![("a", el("Card", serde_json::json!({}), vec![]))]);
        let new_spec = make_spec(
            "a",
            vec![
                ("a", el("Card", serde_json::json!({}), vec!["b"])),
                ("b", el("Button", serde_json::json!({"label": "Go"}), vec![])),
            ],
        );
        let patches = diff_specs(&old, &new_spec);
        assert!(patches.iter().any(|p| matches!(p, SpecPatch::Add { id, .. } if id == "b")));
        assert!(patches
            .iter()
            .any(|p| matches!(p, SpecPatch::Replace { id, .. } if id == "a")));
    }

    #[test]
    fn remove_element() {
        let old = make_spec(
            "a",
            vec![
                ("a", el("Card", serde_json::json!({}), vec!["b"])),
                ("b", el("Button", serde_json::json!({}), vec![])),
            ],
        );
        let new_spec = make_spec("a", vec![("a", el("Card", serde_json::json!({}), vec![]))]);
        let patches = diff_specs(&old, &new_spec);
        assert!(patches
            .iter()
            .any(|p| matches!(p, SpecPatch::Remove { id } if id == "b")));
    }

    #[test]
    fn replace_props() {
        let old = make_spec(
            "a",
            vec![("a", el("Card", serde_json::json!({"title": "Old"}), vec![]))],
        );
        let new_spec = make_spec(
            "a",
            vec![("a", el("Card", serde_json::json!({"title": "New"}), vec![]))],
        );
        let patches = diff_specs(&old, &new_spec);
        assert_eq!(patches.len(), 1);
        assert!(matches!(&patches[0], SpecPatch::Replace { id, .. } if id == "a"));
    }

    #[test]
    fn change_root() {
        let old = make_spec("a", vec![("a", el("Card", serde_json::json!({}), vec![]))]);
        let mut new_spec = old.clone();
        new_spec.root = "b".into();
        new_spec
            .elements
            .insert("b".into(), el("Card", serde_json::json!({}), vec![]));
        let patches = diff_specs(&old, &new_spec);
        assert!(patches
            .iter()
            .any(|p| matches!(p, SpecPatch::SetRoot { root } if root == "b")));
    }

    #[test]
    fn round_trip() {
        let old = make_spec(
            "a",
            vec![
                (
                    "a",
                    el("Card", serde_json::json!({"title": "Old"}), vec!["b"]),
                ),
                ("b", el("Button", serde_json::json!({"label": "Go"}), vec![])),
            ],
        );
        let new_spec = make_spec(
            "a",
            vec![
                (
                    "a",
                    el(
                        "Card",
                        serde_json::json!({"title": "New"}),
                        vec!["b", "c"],
                    ),
                ),
                ("b", el("Button", serde_json::json!({"label": "Go"}), vec![])),
                (
                    "c",
                    el("Metric", serde_json::json!({"value": "100"}), vec![]),
                ),
            ],
        );
        let patches = diff_specs(&old, &new_spec);
        let result = apply_patches(&old, &patches);
        assert_eq!(result.root, new_spec.root);
        assert_eq!(result.elements.len(), new_spec.elements.len());
        for (k, v) in &new_spec.elements {
            let r = result.elements.get(k).unwrap();
            assert_eq!(r.element_type, v.element_type);
            assert_eq!(r.props, v.props);
            assert_eq!(r.children, v.children);
        }
    }

    #[test]
    fn apply_remove_cleans_children() {
        let spec = make_spec(
            "a",
            vec![
                ("a", el("Card", serde_json::json!({}), vec!["b"])),
                ("b", el("Button", serde_json::json!({}), vec![])),
            ],
        );
        let patches = vec![SpecPatch::Remove { id: "b".into() }];
        let result = apply_patches(&spec, &patches);
        assert!(!result.elements.contains_key("b"));
        assert!(result.elements["a"].children.is_empty());
    }
}
