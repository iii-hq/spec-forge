use crate::types::Catalog;

fn is_3d_catalog(catalog: &Catalog) -> bool {
    catalog.components.contains_key("PerspectiveCamera")
        && catalog.components.contains_key("AmbientLight")
}

pub fn build_prompt(user_prompt: &str, catalog: &Catalog) -> String {
    if is_3d_catalog(catalog) {
        build_3d_prompt(user_prompt, catalog)
    } else {
        build_ui_prompt(user_prompt, catalog)
    }
}

fn build_ui_prompt(user_prompt: &str, catalog: &Catalog) -> String {
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

    append_component_listing(&mut prompt, catalog);
    append_action_listing(&mut prompt, catalog);

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

fn build_3d_prompt(user_prompt: &str, catalog: &Catalog) -> String {
    let mut prompt = String::with_capacity(16384);

    prompt.push_str(r##"You are a 3D scene generator that outputs JSONL (one JSON object per line).
You create Three.js scenes using React Three Fiber components via json-render spec format.

OUTPUT FORMAT (JSONL, RFC 6902 JSON Patch):
Output one JSON patch operation per line to build a 3D scene spec progressively.
Each line MUST be a complete, valid JSON object. No markdown, no code fences, no explanation.

The root element should be a Group that contains the entire scene.
Build the scene layer by layer: camera/controls first, then environment/lights, then geometry, then effects.

Example 3D scene output:

{"op":"add","path":"/root","value":"scene"}
{"op":"add","path":"/elements/scene","value":{"type":"Group","props":{"position":[0,0,0]},"children":["camera","controls","env","light-amb","light-dir","subject","shadows","effects"]}}
{"op":"add","path":"/elements/camera","value":{"type":"PerspectiveCamera","props":{"position":[5,3,5],"fov":45,"makeDefault":true},"children":[]}}
{"op":"add","path":"/elements/controls","value":{"type":"OrbitControls","props":{"enableDamping":true,"autoRotate":true},"children":[]}}
{"op":"add","path":"/elements/env","value":{"type":"Environment","props":{"preset":"studio","background":false},"children":[]}}
{"op":"add","path":"/elements/light-amb","value":{"type":"AmbientLight","props":{"intensity":0.4},"children":[]}}
{"op":"add","path":"/elements/light-dir","value":{"type":"DirectionalLight","props":{"position":[5,8,5],"intensity":1.2,"castShadow":true},"children":[]}}
{"op":"add","path":"/elements/subject","value":{"type":"Sphere","props":{"radius":1,"position":[0,1,0],"material":{"color":"#4488ff","metalness":0.8,"roughness":0.2},"castShadow":true},"children":[]}}
{"op":"add","path":"/elements/shadows","value":{"type":"ContactShadows","props":{"opacity":0.6,"blur":2,"position":[0,-0.01,0]},"children":[]}}
{"op":"add","path":"/elements/effects","value":{"type":"EffectComposer","props":{"enabled":true},"children":["bloom"]}}
{"op":"add","path":"/elements/bloom","value":{"type":"Bloom","props":{"intensity":0.5,"luminanceThreshold":0.8,"mipmapBlur":true},"children":[]}}

"##);

    append_component_listing(&mut prompt, catalog);

    prompt.push_str(r##"
3D SCENE RULES:
1. Output ONLY JSONL patches — one JSON object per line, no markdown, no code fences, no text
2. First line MUST set root: {"op":"add","path":"/root","value":"<root-key>"}
3. Root should be a Group containing the full scene graph
4. ALWAYS include: PerspectiveCamera (with makeDefault:true), OrbitControls, at least one light
5. position/rotation/scale are [x,y,z] number arrays — Y is up
6. material is an object: {"color":"#hex","metalness":0-1,"roughness":0-1}
7. Use Environment preset for realistic reflections (studio, city, sunset, etc.)
8. Add ContactShadows for grounded objects, enable castShadow on meshes
9. Wrap animated objects in Float/Spin/Orbit for motion
10. Use EffectComposer with Bloom for emissive/glowing materials
11. Generate 10-25 elements for a rich, visually complete scene
12. Use descriptive keys like "floor-plane", "main-sphere", "key-light"
13. Position objects in world space — ground at Y=0, objects above
14. For product scenes: use Backdrop, ContactShadows, Environment preset="studio"
15. Post-processing (Bloom, Vignette, Glitch) must be children of EffectComposer
"##);

    prompt.push_str("\nUSER REQUEST: ");
    prompt.push_str(user_prompt);
    prompt.push_str("\n\nOutput JSONL patches now:\n");

    prompt
}

fn append_component_listing(prompt: &mut String, catalog: &Catalog) {
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
}

fn append_action_listing(prompt: &mut String, catalog: &Catalog) {
    if !catalog.actions.is_empty() {
        prompt.push_str("\nAVAILABLE ACTIONS:\n");
        for (name, def) in &catalog.actions {
            prompt.push_str(&format!("- {}: {}\n", name, def.description));
        }
    }
}

pub fn build_refine_prompt(
    user_prompt: &str,
    current_spec: &crate::types::UISpec,
    catalog: &Catalog,
) -> String {
    let mut prompt = String::with_capacity(8192);

    let mode = if is_3d_catalog(catalog) {
        "3D scene"
    } else {
        "UI"
    };

    prompt.push_str(&format!(
        r#"You are a {mode} editor. Given an existing spec and a change request, output JSONL patches (RFC 6902) to modify the spec.

OPERATIONS:
- Add new element: {{"op":"add","path":"/elements/<key>","value":{{"type":"...","props":{{...}},"children":[...]}}}}
- Replace element: {{"op":"replace","path":"/elements/<key>","value":{{"type":"...","props":{{...}},"children":[...]}}}}
- Remove element: {{"op":"remove","path":"/elements/<key>"}}
- Change root: {{"op":"replace","path":"/root","value":"<new-root-key>"}}

Output ONLY JSONL patches — one JSON object per line. No text, no markdown.

"#
    ));

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

    #[test]
    fn three_d_catalog_uses_3d_prompt() {
        let catalog = crate::catalogs::three_d();
        let result = build_prompt("product showroom", &catalog);
        assert!(result.contains("3D scene generator"));
        assert!(result.contains("Three.js"));
        assert!(result.contains("PerspectiveCamera"));
        assert!(result.contains("3D SCENE RULES"));
        assert!(result.contains("Y is up"));
    }

    #[test]
    fn ui_catalog_uses_ui_prompt() {
        let catalog = crate::catalogs::get_preset("dashboard").unwrap();
        let result = build_prompt("sales metrics", &catalog);
        assert!(result.contains("UI generator"));
        assert!(!result.contains("3D scene generator"));
    }

    #[test]
    fn three_d_prompt_has_scene_example() {
        let catalog = crate::catalogs::three_d();
        let result = build_prompt("test", &catalog);
        assert!(result.contains("Environment"));
        assert!(result.contains("ContactShadows"));
        assert!(result.contains("EffectComposer"));
    }

    #[test]
    fn three_d_refine_says_3d_scene() {
        let spec = UISpec {
            root: "scene".into(),
            elements: Default::default(),
        };
        let catalog = crate::catalogs::three_d();
        let result = build_refine_prompt("add bloom", &spec, &catalog);
        assert!(result.contains("3D scene editor"));
    }
}
