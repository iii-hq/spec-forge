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

const THREE_D_LIGHTS: &[&str] = &[
    "AmbientLight",
    "DirectionalLight",
    "PointLight",
    "SpotLight",
];

const THREE_D_CAMERAS: &[&str] = &["PerspectiveCamera"];

fn is_3d_catalog(catalog: &Catalog) -> bool {
    catalog.components.contains_key("PerspectiveCamera")
        && catalog.components.contains_key("AmbientLight")
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

    if is_3d_catalog(catalog) {
        validate_3d_scene(spec, &reachable, &mut errors);
    }

    errors
}

fn validate_3d_scene(
    spec: &UISpec,
    reachable: &HashSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    let reachable_elements: Vec<(&String, &UIElement)> = spec
        .elements
        .iter()
        .filter(|(id, _)| reachable.contains(id.as_str()))
        .collect();

    let has_camera = reachable_elements
        .iter()
        .any(|(_, e)| THREE_D_CAMERAS.contains(&e.element_type.as_str()));
    let has_light = reachable_elements
        .iter()
        .any(|(_, e)| THREE_D_LIGHTS.contains(&e.element_type.as_str()));

    if !has_camera {
        errors.push(ValidationError {
            element_id: "__scene__".into(),
            message: "3D scene missing PerspectiveCamera — scene won't render".into(),
        });
    }
    if !has_light {
        errors.push(ValidationError {
            element_id: "__scene__".into(),
            message: "3D scene has no lights (AmbientLight, DirectionalLight, PointLight, or SpotLight) — objects will be dark".into(),
        });
    }

    let postfx_children: &[&str] = &["Bloom", "Glitch", "Vignette"];
    for (id, el) in &reachable_elements {
        if postfx_children.contains(&el.element_type.as_str()) {
            let is_child_of_composer = reachable_elements.iter().any(|(_, parent)| {
                parent.element_type == "EffectComposer"
                    && parent.children.iter().any(|c| c == *id)
            });
            if !is_child_of_composer {
                errors.push(ValidationError {
                    element_id: (*id).clone(),
                    message: format!(
                        "{} must be a child of EffectComposer",
                        el.element_type
                    ),
                });
            }
        }
    }
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
        assert_eq!(errors.len(), 3, "Expected exactly 3 errors (bad ref + unknown type + orphan), got: {:?}", errors);
    }

    #[test]
    fn three_d_scene_missing_camera_and_light() {
        let catalog = crate::catalogs::three_d();
        let mut elements = HashMap::new();
        elements.insert(
            "scene".into(),
            UIElement {
                element_type: "Group".into(),
                props: serde_json::json!({}),
                children: vec!["box-1".into()],
            },
        );
        elements.insert(
            "box-1".into(),
            UIElement {
                element_type: "Box".into(),
                props: serde_json::json!({"position": [0,1,0]}),
                children: vec![],
            },
        );
        let spec = UISpec {
            root: "scene".into(),
            elements,
        };
        let errors = validate_spec(&spec, &catalog);
        let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("PerspectiveCamera")),
            "Should warn about missing camera"
        );
        assert!(
            messages.iter().any(|m| m.contains("no lights")),
            "Should warn about missing lights"
        );
    }

    #[test]
    fn three_d_valid_scene_passes() {
        let catalog = crate::catalogs::three_d();
        let mut elements = HashMap::new();
        elements.insert(
            "scene".into(),
            UIElement {
                element_type: "Group".into(),
                props: serde_json::json!({}),
                children: vec![
                    "cam".into(),
                    "light".into(),
                    "sphere".into(),
                ],
            },
        );
        elements.insert(
            "cam".into(),
            UIElement {
                element_type: "PerspectiveCamera".into(),
                props: serde_json::json!({"position": [5,3,5], "fov": 45, "makeDefault": true}),
                children: vec![],
            },
        );
        elements.insert(
            "light".into(),
            UIElement {
                element_type: "AmbientLight".into(),
                props: serde_json::json!({"intensity": 0.5}),
                children: vec![],
            },
        );
        elements.insert(
            "sphere".into(),
            UIElement {
                element_type: "Sphere".into(),
                props: serde_json::json!({"radius": 1, "position": [0,1,0]}),
                children: vec![],
            },
        );
        let spec = UISpec {
            root: "scene".into(),
            elements,
        };
        let errors = validate_spec(&spec, &catalog);
        assert!(errors.is_empty(), "Got: {:?}", errors);
    }

    #[test]
    fn three_d_bloom_outside_effect_composer() {
        let catalog = crate::catalogs::three_d();
        let mut elements = HashMap::new();
        elements.insert(
            "scene".into(),
            UIElement {
                element_type: "Group".into(),
                props: serde_json::json!({}),
                children: vec!["cam".into(), "light".into(), "bloom".into()],
            },
        );
        elements.insert(
            "cam".into(),
            UIElement {
                element_type: "PerspectiveCamera".into(),
                props: serde_json::json!({"position": [5,3,5], "makeDefault": true}),
                children: vec![],
            },
        );
        elements.insert(
            "light".into(),
            UIElement {
                element_type: "AmbientLight".into(),
                props: serde_json::json!({"intensity": 1.0}),
                children: vec![],
            },
        );
        elements.insert(
            "bloom".into(),
            UIElement {
                element_type: "Bloom".into(),
                props: serde_json::json!({"intensity": 1.0}),
                children: vec![],
            },
        );
        let spec = UISpec {
            root: "scene".into(),
            elements,
        };
        let errors = validate_spec(&spec, &catalog);
        assert!(
            errors.iter().any(|e| e.message.contains("EffectComposer")),
            "Bloom outside EffectComposer should warn"
        );
    }
}
