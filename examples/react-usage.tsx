/**
 * Example: Using iii-render with json-render's React <Render> component
 *
 * npm install @iii-dev/render-client @anthropic-ai/json-render-react
 */

import { useState, useCallback } from "react";
import { Render } from "@anthropic-ai/json-render-react";
import { IIIRenderClient } from "@iii-dev/render-client";
import type { UISpec, Catalog } from "@iii-dev/render-client";

const client = new IIIRenderClient({ baseUrl: "http://localhost:3112" });

const catalog: Catalog = {
  components: {
    Card: { description: "Container card", props: { title: "string" }, children: true },
    Metric: { description: "Metric display", props: { label: "string", value: "string" } },
    Button: { description: "Button", props: { label: "string", variant: "string" } },
    Input: { description: "Input field", props: { placeholder: "string", type: "string" } },
    Text: { description: "Text content", props: { content: "string" } },
    Table: { description: "Data table", props: { columns: "string[]", rows: "array" } },
  },
  actions: {
    navigate: { description: "Navigate to a route" },
    submit: { description: "Submit form data" },
  },
};

export function GenerativeUI() {
  const [spec, setSpec] = useState<UISpec | null>(null);
  const [prompt, setPrompt] = useState("");
  const [loading, setLoading] = useState(false);
  const [refineText, setRefineText] = useState("");

  const generate = useCallback(async () => {
    setLoading(true);
    try {
      const res = await client.generate(prompt, catalog);
      setSpec(res.spec);
    } finally {
      setLoading(false);
    }
  }, [prompt]);

  const refine = useCallback(async () => {
    if (!spec) return;
    setLoading(true);
    try {
      const res = await client.refine(refineText, spec, catalog);
      setSpec(res.spec);
      setRefineText("");
    } finally {
      setLoading(false);
    }
  }, [spec, refineText]);

  const streamGenerate = useCallback(async () => {
    setLoading(true);
    setSpec(null);

    const partialSpec: UISpec = { root: "", elements: {} };

    for await (const event of client.stream(prompt, catalog)) {
      switch (event.type) {
        case "root":
          partialSpec.root = event.root!;
          break;
        case "element":
          partialSpec.elements[event.id!] = event.element!;
          setSpec({ ...partialSpec });
          break;
        case "done":
          setSpec(event.spec!);
          break;
      }
    }
    setLoading(false);
  }, [prompt]);

  return (
    <div style={{ display: "flex", gap: 24, padding: 24 }}>
      <div style={{ width: 400 }}>
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="Describe a UI..."
          style={{ width: "100%", height: 120 }}
        />

        <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
          <button onClick={generate} disabled={loading}>
            Generate
          </button>
          <button onClick={streamGenerate} disabled={loading}>
            Stream
          </button>
        </div>

        {spec && (
          <div style={{ marginTop: 16 }}>
            <input
              value={refineText}
              onChange={(e) => setRefineText(e.target.value)}
              placeholder="Refine: add a header..."
            />
            <button onClick={refine} disabled={loading || !refineText}>
              Refine
            </button>
          </div>
        )}
      </div>

      <div style={{ flex: 1 }}>
        {spec ? (
          <Render spec={spec} catalog={catalog} />
        ) : (
          <p style={{ color: "#888" }}>Generate a UI to see it here</p>
        )}
      </div>
    </div>
  );
}
