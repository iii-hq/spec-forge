use crate::types::{ActionDef, Catalog, ComponentDef};
use std::collections::BTreeMap;

pub fn get_preset(name: &str) -> Option<Catalog> {
    match name {
        "minimal" => Some(minimal()),
        "dashboard" => Some(dashboard()),
        "form" => Some(form()),
        "ecommerce" => Some(ecommerce()),
        "3d" | "three" | "3d-scene" => Some(three_d()),
        "3d-product" => Some(three_d_product()),
        _ => None,
    }
}

pub fn list_presets() -> Vec<&'static str> {
    vec!["minimal", "dashboard", "form", "ecommerce", "3d", "3d-product"]
}

fn c(description: &str, props: serde_json::Value, children: bool) -> ComponentDef {
    ComponentDef {
        description: description.into(),
        props,
        children,
    }
}

fn a(description: &str) -> ActionDef {
    ActionDef {
        description: description.into(),
    }
}

fn minimal() -> Catalog {
    let mut components = BTreeMap::new();
    components.insert("Stack".into(), c("Flex container", serde_json::json!({"direction": "vertical|horizontal", "gap": "number"}), true));
    components.insert("Card".into(), c("Container card", serde_json::json!({"title": "string"}), true));
    components.insert("Heading".into(), c("Section heading", serde_json::json!({"level": "1|2|3", "text": "string"}), false));
    components.insert("Text".into(), c("Text paragraph", serde_json::json!({"content": "string"}), false));
    components.insert("Button".into(), c("Clickable button", serde_json::json!({"label": "string", "variant": "primary|secondary"}), false));
    components.insert("Input".into(), c("Text input", serde_json::json!({"placeholder": "string", "type": "text|email|password|number", "label": "string"}), false));

    let mut actions = BTreeMap::new();
    actions.insert("save".into(), a("Save changes"));

    Catalog { components, actions }
}

fn dashboard() -> Catalog {
    let mut components = BTreeMap::new();
    components.insert("Stack".into(), c("Flex container", serde_json::json!({"direction": "vertical|horizontal", "gap": "number"}), true));
    components.insert("Card".into(), c("Container card", serde_json::json!({"title": "string"}), true));
    components.insert("Grid".into(), c("Grid layout", serde_json::json!({"columns": "number", "gap": "number"}), true));
    components.insert("Heading".into(), c("Section heading", serde_json::json!({"level": "1|2|3", "text": "string"}), false));
    components.insert("Metric".into(), c("Display a KPI metric", serde_json::json!({"label": "string", "value": "string", "format": "string|null", "trend": "up|down|neutral"}), false));
    components.insert("Table".into(), c("Data table", serde_json::json!({"headers": "string[]", "rows": "string[][]"}), false));
    components.insert("Chart".into(), c("Chart visualization", serde_json::json!({"type": "bar|line|pie", "data": "object", "title": "string"}), false));
    components.insert("Button".into(), c("Clickable button", serde_json::json!({"label": "string", "variant": "primary|secondary", "action": "string"}), false));
    components.insert("Text".into(), c("Text paragraph", serde_json::json!({"content": "string"}), false));
    components.insert("Badge".into(), c("Status badge", serde_json::json!({"label": "string", "color": "green|red|yellow|blue"}), false));
    components.insert("Divider".into(), c("Visual separator", serde_json::json!({}), false));
    components.insert("Input".into(), c("Text input field", serde_json::json!({"label": "string", "placeholder": "string", "type": "text|email|password|number"}), false));

    let mut actions = BTreeMap::new();
    actions.insert("export_report".into(), a("Export dashboard to PDF"));
    actions.insert("refresh_data".into(), a("Refresh all data"));
    actions.insert("save".into(), a("Save changes"));
    actions.insert("filter".into(), a("Apply data filter"));

    Catalog { components, actions }
}

fn form() -> Catalog {
    let mut components = BTreeMap::new();
    components.insert("Stack".into(), c("Flex container", serde_json::json!({"direction": "vertical|horizontal", "gap": "number"}), true));
    components.insert("Card".into(), c("Container card", serde_json::json!({"title": "string"}), true));
    components.insert("Heading".into(), c("Section heading", serde_json::json!({"level": "1|2|3", "text": "string"}), false));
    components.insert("Input".into(), c("Text input", serde_json::json!({"placeholder": "string", "type": "text|email|password|number", "label": "string"}), false));
    components.insert("Textarea".into(), c("Multi-line text input", serde_json::json!({"placeholder": "string", "label": "string", "rows": "number"}), false));
    components.insert("Select".into(), c("Dropdown selector", serde_json::json!({"label": "string", "options": "string[]", "placeholder": "string"}), false));
    components.insert("Checkbox".into(), c("Checkbox toggle", serde_json::json!({"label": "string"}), false));
    components.insert("Radio".into(), c("Radio button group", serde_json::json!({"label": "string", "options": "string[]"}), false));
    components.insert("Button".into(), c("Button", serde_json::json!({"label": "string", "variant": "primary|secondary|destructive"}), false));
    components.insert("Text".into(), c("Text paragraph", serde_json::json!({"content": "string"}), false));
    components.insert("Divider".into(), c("Visual separator", serde_json::json!({}), false));
    components.insert("Badge".into(), c("Status indicator", serde_json::json!({"label": "string", "color": "green|red|yellow"}), false));

    let mut actions = BTreeMap::new();
    actions.insert("submit".into(), a("Submit form"));
    actions.insert("validate".into(), a("Validate form fields"));
    actions.insert("reset".into(), a("Reset form"));

    Catalog { components, actions }
}

fn ecommerce() -> Catalog {
    let mut components = BTreeMap::new();
    components.insert("Stack".into(), c("Flex container", serde_json::json!({"direction": "vertical|horizontal", "gap": "number"}), true));
    components.insert("Grid".into(), c("Grid layout", serde_json::json!({"columns": "number", "gap": "number"}), true));
    components.insert("Card".into(), c("Container card", serde_json::json!({"title": "string"}), true));
    components.insert("Heading".into(), c("Section heading", serde_json::json!({"level": "1|2|3", "text": "string"}), false));
    components.insert("Image".into(), c("Image display", serde_json::json!({"src": "string", "alt": "string"}), false));
    components.insert("Text".into(), c("Text paragraph", serde_json::json!({"content": "string"}), false));
    components.insert("Button".into(), c("Clickable button", serde_json::json!({"label": "string", "variant": "primary|secondary|destructive"}), false));
    components.insert("Metric".into(), c("Display a value", serde_json::json!({"label": "string", "value": "string"}), false));
    components.insert("Badge".into(), c("Status badge", serde_json::json!({"label": "string", "color": "green|red|yellow|blue"}), false));
    components.insert("Divider".into(), c("Visual separator", serde_json::json!({}), false));
    components.insert("List".into(), c("Ordered/unordered list", serde_json::json!({"items": "string[]"}), false));

    Catalog { components, actions: BTreeMap::new() }
}

pub fn three_d() -> Catalog {
    let mut components = BTreeMap::new();

    components.insert("Box".into(), c(
        "Box mesh (default 1x1x1)",
        serde_json::json!({"width": "number", "height": "number", "depth": "number", "position": "[x,y,z]", "rotation": "[x,y,z]", "scale": "[x,y,z]", "material": {"color": "string", "metalness": "number(0-1)", "roughness": "number(0-1)", "emissive": "string", "opacity": "number(0-1)", "transparent": "boolean", "wireframe": "boolean"}, "castShadow": "boolean", "receiveShadow": "boolean"}),
        false,
    ));
    components.insert("Sphere".into(), c(
        "Sphere mesh",
        serde_json::json!({"radius": "number", "widthSegments": "number", "heightSegments": "number", "position": "[x,y,z]", "rotation": "[x,y,z]", "scale": "[x,y,z]", "material": {"color": "string", "metalness": "number(0-1)", "roughness": "number(0-1)", "emissive": "string", "opacity": "number(0-1)"}, "castShadow": "boolean", "receiveShadow": "boolean"}),
        false,
    ));
    components.insert("Cylinder".into(), c(
        "Cylinder mesh",
        serde_json::json!({"radiusTop": "number", "radiusBottom": "number", "height": "number", "radialSegments": "number", "position": "[x,y,z]", "rotation": "[x,y,z]", "material": {"color": "string", "metalness": "number(0-1)", "roughness": "number(0-1)"}, "castShadow": "boolean"}),
        false,
    ));
    components.insert("Cone".into(), c(
        "Cone mesh",
        serde_json::json!({"radius": "number", "height": "number", "radialSegments": "number", "position": "[x,y,z]", "rotation": "[x,y,z]", "material": {"color": "string", "metalness": "number(0-1)", "roughness": "number(0-1)"}, "castShadow": "boolean"}),
        false,
    ));
    components.insert("Torus".into(), c(
        "Torus (donut) mesh",
        serde_json::json!({"radius": "number", "tube": "number", "position": "[x,y,z]", "rotation": "[x,y,z]", "material": {"color": "string", "metalness": "number(0-1)", "roughness": "number(0-1)"}, "castShadow": "boolean"}),
        false,
    ));
    components.insert("Plane".into(), c(
        "Flat plane mesh",
        serde_json::json!({"width": "number", "height": "number", "position": "[x,y,z]", "rotation": "[x,y,z]", "material": {"color": "string", "opacity": "number(0-1)"}, "receiveShadow": "boolean"}),
        true,
    ));
    components.insert("Capsule".into(), c(
        "Capsule mesh (cylinder + hemispherical caps)",
        serde_json::json!({"radius": "number", "length": "number", "position": "[x,y,z]", "rotation": "[x,y,z]", "material": {"color": "string", "metalness": "number(0-1)", "roughness": "number(0-1)"}, "castShadow": "boolean"}),
        false,
    ));

    components.insert("TorusKnot".into(), c(
        "Intricate knot shape via p/q parameters",
        serde_json::json!({"radius": "number", "tube": "number", "p": "number", "q": "number", "position": "[x,y,z]", "material": {"color": "string", "metalness": "number(0-1)", "roughness": "number(0-1)"}, "castShadow": "boolean"}),
        false,
    ));
    components.insert("RoundedBox".into(), c(
        "Box with rounded edges (product-style)",
        serde_json::json!({"width": "number", "height": "number", "depth": "number", "radius": "number", "smoothness": "number", "position": "[x,y,z]", "material": {"color": "string", "metalness": "number(0-1)", "roughness": "number(0-1)"}, "castShadow": "boolean"}),
        false,
    ));

    components.insert("AmbientLight".into(), c(
        "Uniform scene illumination",
        serde_json::json!({"color": "string", "intensity": "number"}),
        false,
    ));
    components.insert("DirectionalLight".into(), c(
        "Sunlight-style directional light",
        serde_json::json!({"position": "[x,y,z]", "intensity": "number", "color": "string", "castShadow": "boolean"}),
        false,
    ));
    components.insert("PointLight".into(), c(
        "Light radiating from a point in all directions",
        serde_json::json!({"position": "[x,y,z]", "intensity": "number", "color": "string", "distance": "number", "decay": "number"}),
        false,
    ));
    components.insert("SpotLight".into(), c(
        "Cone of light from a point",
        serde_json::json!({"position": "[x,y,z]", "intensity": "number", "color": "string", "angle": "number", "penumbra": "number", "castShadow": "boolean"}),
        false,
    ));

    components.insert("GlassSphere".into(), c(
        "Photorealistic glass sphere with transmission and refraction",
        serde_json::json!({"radius": "number", "position": "[x,y,z]", "transmission": "number(0-1)", "thickness": "number", "chromaticAberration": "number", "ior": "number", "color": "string", "castShadow": "boolean"}),
        false,
    ));
    components.insert("GlassBox".into(), c(
        "Photorealistic glass cuboid",
        serde_json::json!({"width": "number", "height": "number", "depth": "number", "position": "[x,y,z]", "transmission": "number(0-1)", "thickness": "number", "chromaticAberration": "number", "ior": "number", "color": "string"}),
        false,
    ));
    components.insert("DistortSphere".into(), c(
        "Animated distorting liquid-metal sphere",
        serde_json::json!({"radius": "number", "position": "[x,y,z]", "speed": "number", "distort": "number", "metalness": "number(0-1)", "roughness": "number(0-1)", "color": "string"}),
        false,
    ));

    components.insert("Environment".into(), c(
        "HDRI environment map for scene lighting and reflections",
        serde_json::json!({"preset": "apartment|city|dawn|forest|lobby|night|park|studio|sunset|warehouse", "background": "boolean", "blur": "number(0-1)", "intensity": "number"}),
        false,
    ));
    components.insert("Fog".into(), c(
        "Linear fog effect",
        serde_json::json!({"color": "string", "near": "number", "far": "number"}),
        false,
    ));
    components.insert("GridHelper".into(), c(
        "Visual reference grid on XZ plane",
        serde_json::json!({"size": "number", "divisions": "number", "color": "string"}),
        false,
    ));

    components.insert("Sparkles".into(), c(
        "Floating particle sparkles (magic, snow, ambient)",
        serde_json::json!({"count": "number", "speed": "number", "opacity": "number(0-1)", "color": "string", "size": "number", "scale": "[x,y,z]"}),
        false,
    ));
    components.insert("Stars".into(), c(
        "Starfield background (thousands of stars in a sphere)",
        serde_json::json!({"radius": "number", "depth": "number", "count": "number", "factor": "number", "fade": "boolean"}),
        false,
    ));
    components.insert("Sky".into(), c(
        "Procedural sky with sun, haze, and atmospheric scattering",
        serde_json::json!({"sunPosition": "[x,y,z]", "rayleigh": "number", "turbidity": "number"}),
        false,
    ));
    components.insert("Cloud".into(), c(
        "Volumetric cloud",
        serde_json::json!({"segments": "number", "volume": "number", "speed": "number", "opacity": "number(0-1)", "position": "[x,y,z]"}),
        false,
    ));

    components.insert("ContactShadows".into(), c(
        "Soft contact shadows on ground plane",
        serde_json::json!({"opacity": "number(0-1)", "blur": "number", "width": "number", "height": "number", "position": "[x,y,z]"}),
        false,
    ));
    components.insert("Float".into(), c(
        "Gentle floating/bobbing animation wrapper",
        serde_json::json!({"speed": "number", "rotationIntensity": "number", "floatIntensity": "number"}),
        true,
    ));
    components.insert("ReflectorPlane".into(), c(
        "Real-time mirror reflections on a plane",
        serde_json::json!({"mirror": "number(0-1)", "blur": "[x,y]", "resolution": "number", "mixStrength": "number", "position": "[x,y,z]", "rotation": "[x,y,z]"}),
        false,
    ));
    components.insert("Backdrop".into(), c(
        "Curved studio backdrop for product scenes",
        serde_json::json!({"floor": "number", "segments": "number", "receiveShadow": "boolean", "position": "[x,y,z]", "scale": "[x,y,z]"}),
        true,
    ));

    components.insert("WarpTunnel".into(), c(
        "Animated neon tunnel flythrough (hyperspace effect)",
        serde_json::json!({"ringCount": "number", "radius": "number", "length": "number", "speed": "number", "color1": "string", "color2": "string"}),
        false,
    ));
    components.insert("Spin".into(), c(
        "Continuous rotation around an axis",
        serde_json::json!({"speed": "number", "axis": "x|y|z"}),
        true,
    ));
    components.insert("Orbit".into(), c(
        "Orbits children around Y axis at given radius",
        serde_json::json!({"speed": "number", "radius": "number", "tilt": "number"}),
        true,
    ));
    components.insert("Pulse".into(), c(
        "Pulses children scale between min/max",
        serde_json::json!({"speed": "number", "min": "number", "max": "number"}),
        true,
    ));
    components.insert("CameraShake".into(), c(
        "Camera vibration effect",
        serde_json::json!({"intensity": "number", "maxYaw": "number", "maxPitch": "number", "maxRoll": "number"}),
        false,
    ));

    components.insert("MeshPortalMaterial".into(), c(
        "Renders children into a portal surface (applied to a Plane)",
        serde_json::json!({"blend": "number(0-1)", "blur": "number", "resolution": "number"}),
        true,
    ));
    components.insert("HtmlLabel".into(), c(
        "Real HTML/DOM text floating in 3D space",
        serde_json::json!({"text": "string", "position": "[x,y,z]", "transform": "boolean", "distanceFactor": "number", "color": "string", "fontSize": "number"}),
        false,
    ));

    components.insert("EffectComposer".into(), c(
        "Wrapper for post-processing effects",
        serde_json::json!({"enabled": "boolean", "multisampling": "number"}),
        true,
    ));
    components.insert("Bloom".into(), c(
        "Glow effect for bright/emissive materials",
        serde_json::json!({"intensity": "number", "luminanceThreshold": "number", "mipmapBlur": "boolean"}),
        false,
    ));
    components.insert("Glitch".into(), c(
        "Cyberpunk glitch distortion effect",
        serde_json::json!({"delay": "[min,max]", "duration": "[min,max]", "strength": "[x,y]", "active": "boolean"}),
        false,
    ));
    components.insert("Vignette".into(), c(
        "Darkened corner vignette",
        serde_json::json!({"offset": "number", "darkness": "number"}),
        false,
    ));

    components.insert("PerspectiveCamera".into(), c(
        "Camera with perspective projection",
        serde_json::json!({"position": "[x,y,z]", "fov": "number", "near": "number", "far": "number", "makeDefault": "boolean"}),
        false,
    ));
    components.insert("OrbitControls".into(), c(
        "Interactive orbit/zoom/pan mouse controls",
        serde_json::json!({"enableDamping": "boolean", "autoRotate": "boolean", "autoRotateSpeed": "number", "minDistance": "number", "maxDistance": "number", "target": "[x,y,z]"}),
        false,
    ));

    components.insert("Group".into(), c(
        "Container for grouping children with shared transform",
        serde_json::json!({"position": "[x,y,z]", "rotation": "[x,y,z]", "scale": "[x,y,z]"}),
        true,
    ));

    components.insert("Model".into(), c(
        "GLTF/GLB 3D model loader",
        serde_json::json!({"url": "string", "position": "[x,y,z]", "rotation": "[x,y,z]", "scale": "[x,y,z]"}),
        false,
    ));

    components.insert("Text3D".into(), c(
        "3D SDF text rendered in the scene",
        serde_json::json!({"text": "string", "fontSize": "number", "color": "string", "position": "[x,y,z]", "anchorX": "left|center|right", "anchorY": "top|middle|bottom"}),
        false,
    ));

    Catalog { components, actions: BTreeMap::new() }
}

fn three_d_product() -> Catalog {
    let full = three_d();
    let keep: std::collections::HashSet<&str> = [
        "Box", "Sphere", "Cylinder", "Plane", "RoundedBox",
        "GlassSphere", "GlassBox",
        "AmbientLight", "DirectionalLight", "SpotLight",
        "Environment", "ContactShadows", "Float", "ReflectorPlane", "Backdrop",
        "Spin", "Orbit",
        "EffectComposer", "Bloom", "Vignette",
        "PerspectiveCamera", "OrbitControls",
        "Group", "Text3D", "HtmlLabel",
    ].into_iter().collect();

    let components = full.components.into_iter()
        .filter(|(k, _)| keep.contains(k.as_str()))
        .collect();

    Catalog { components, actions: BTreeMap::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_d_catalog_has_43_components() {
        let catalog = three_d();
        assert_eq!(catalog.components.len(), 43);
    }

    #[test]
    fn three_d_product_is_subset() {
        let full = three_d();
        let product = three_d_product();
        assert!(product.components.len() < full.components.len());
        for name in product.components.keys() {
            assert!(full.components.contains_key(name), "{} not in full catalog", name);
        }
    }

    #[test]
    fn all_presets_load() {
        for name in list_presets() {
            assert!(get_preset(name).is_some(), "Preset {} failed to load", name);
        }
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(get_preset("nonexistent").is_none());
    }

    #[test]
    fn three_d_has_essential_scene_components() {
        let catalog = three_d();
        let essentials = [
            "Box", "Sphere", "Cylinder", "Cone", "Torus", "Plane", "Capsule",
            "AmbientLight", "DirectionalLight", "PointLight", "SpotLight",
            "PerspectiveCamera", "OrbitControls", "Group", "Environment",
            "GlassSphere", "DistortSphere", "Float", "Spin",
            "EffectComposer", "Bloom", "Text3D", "Model",
        ];
        for name in essentials {
            assert!(catalog.components.contains_key(name), "Missing: {}", name);
        }
    }
}
