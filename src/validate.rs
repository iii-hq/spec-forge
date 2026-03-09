use crate::types::{Catalog, UIElement, UISpec};
use std::collections::HashSet;

#[derive(Debug)]
pub struct ValidationError {
    pub element_id: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.element_id, self.message)
    }
}

pub fn validate_spec(spec: &UISpec, catalog: &Catalog) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let component_names: HashSet<&str> = catalog.components.keys().map(|s| s.as_str()).collect();

    if !spec.elements.contains_key(&spec.root) {
        errors.push(ValidationError {
            element_id: spec.root.clone(),
            message: format!("Root element '{}' not found in elements", spec.root),
        });
        return errors;
    }

    for (id, element) in &spec.elements {
        if !component_names.contains(element.element_type.as_str()) {
            errors.push(ValidationError {
                element_id: id.clone(),
                message: format!(
                    "Component '{}' not in catalog. Available: [{}]",
                    element.element_type,
                    component_names
                        .iter()
                        .copied()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }

        for child_id in &element.children {
            if !spec.elements.contains_key(child_id) {
                errors.push(ValidationError {
                    element_id: id.clone(),
                    message: format!("Child '{}' not found in elements", child_id),
                });
            }
        }
    }

    let reachable = collect_reachable(&spec.root, &spec.elements);
    for id in spec.elements.keys() {
        if !reachable.contains(id.as_str()) {
            errors.push(ValidationError {
                element_id: id.clone(),
                message: "Orphaned element — not reachable from root".into(),
            });
        }
    }

    errors
}

fn collect_reachable<'a>(
    root: &'a str,
    elements: &'a std::collections::HashMap<String, UIElement>,
) -> HashSet<&'a str> {
    let mut visited = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if visited.insert(id) {
            if let Some(el) = elements.get(id) {
                for child in &el.children {
                    stack.push(child.as_str());
                }
            }
        }
    }
    visited
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::collections::{BTreeMap, HashMap};

    fn test_catalog() -> Catalog {
        let mut components = BTreeMap::new();
        components.insert(
            "Card".into(),
            ComponentDef {
                description: "A card container".into(),
                props: serde_json::json!({}),
                children: true,
            },
        );
        components.insert(
            "Metric".into(),
            ComponentDef {
                description: "Display a metric".into(),
                props: serde_json::json!({"label": "string", "value": "string"}),
                children: false,
            },
        );
        components.insert(
            "Button".into(),
            ComponentDef {
                description: "Clickable button".into(),
                props: serde_json::json!({"label": "string"}),
                children: false,
            },
        );
        Catalog {
            components,
            actions: BTreeMap::new(),
        }
    }

    #[test]
    fn valid_spec_passes() {
        let catalog = test_catalog();
        let mut elements = HashMap::new();
        elements.insert(
            "card-1".into(),
            UIElement {
                element_type: "Card".into(),
                props: serde_json::json!({"title": "Dashboard"}),
                children: vec!["metric-1".into(), "button-1".into()],
            },
        );
        elements.insert(
            "metric-1".into(),
            UIElement {
                element_type: "Metric".into(),
                props: serde_json::json!({"label": "Revenue", "value": "$1.2M"}),
                children: vec![],
            },
        );
        elements.insert(
            "button-1".into(),
            UIElement {
                element_type: "Button".into(),
                props: serde_json::json!({"label": "Export"}),
                children: vec![],
            },
        );
        let spec = UISpec {
            root: "card-1".into(),
            elements,
        };
        let errors = validate_spec(&spec, &catalog);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn unknown_component_type() {
        let catalog = test_catalog();
        let mut elements = HashMap::new();
        elements.insert(
            "chart-1".into(),
            UIElement {
                element_type: "Chart".into(),
                props: serde_json::json!({}),
                children: vec![],
            },
        );
        let spec = UISpec {
            root: "chart-1".into(),
            elements,
        };
        let errors = validate_spec(&spec, &catalog);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Chart"));
        assert!(errors[0].message.contains("not in catalog"));
    }

    #[test]
    fn missing_root() {
        let catalog = test_catalog();
        let spec = UISpec {
            root: "nonexistent".into(),
            elements: HashMap::new(),
        };
        let errors = validate_spec(&spec, &catalog);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("not found"));
    }

    #[test]
    fn missing_child_reference() {
        let catalog = test_catalog();
        let mut elements = HashMap::new();
        elements.insert(
            "card-1".into(),
            UIElement {
                element_type: "Card".into(),
                props: serde_json::json!({}),
                children: vec!["ghost-1".into()],
            },
        );
        let spec = UISpec {
            root: "card-1".into(),
            elements,
        };
        let errors = validate_spec(&spec, &catalog);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("ghost-1"));
    }

    #[test]
    fn orphaned_element() {
        let catalog = test_catalog();
        let mut elements = HashMap::new();
        elements.insert(
            "card-1".into(),
            UIElement {
                element_type: "Card".into(),
                props: serde_json::json!({}),
                children: vec![],
            },
        );
        elements.insert(
            "orphan-1".into(),
            UIElement {
                element_type: "Button".into(),
                props: serde_json::json!({"label": "Lost"}),
                children: vec![],
            },
        );
        let spec = UISpec {
            root: "card-1".into(),
            elements,
        };
        let errors = validate_spec(&spec, &catalog);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Orphaned"));
        assert_eq!(errors[0].element_id, "orphan-1");
    }

    #[test]
    fn multiple_errors() {
        let catalog = test_catalog();
        let mut elements = HashMap::new();
        elements.insert(
            "card-1".into(),
            UIElement {
                element_type: "Card".into(),
                props: serde_json::json!({}),
                children: vec!["bad-ref".into()],
            },
        );
        elements.insert(
            "orphan-1".into(),
            UIElement {
                element_type: "FakeComponent".into(),
                props: serde_json::json!({}),
                children: vec![],
            },
        );
        let spec = UISpec {
            root: "card-1".into(),
            elements,
        };
        let errors = validate_spec(&spec, &catalog);
        assert!(errors.len() >= 3); // bad ref + unknown type + orphan
    }
}
