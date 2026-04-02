/**
 * Example: Using spec-forge with json-render's React <Render> component
 *
 * npm install @iii-hq/spec-forge @json-render/react iii-browser-sdk
 */

import { useState, useCallback, useEffect, useRef } from "react";
import { Render } from "@json-render/react";
import { createSpecForge, type SpecForge } from "@iii-hq/spec-forge";
import type { SpecForgeCatalog } from "@iii-hq/spec-forge";

const catalog: SpecForgeCatalog = {
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

/**
 * Assumes `iii` is a pre-initialized iii-browser-sdk connection
 * (e.g. from `await initIII('ws://localhost:49135')`)
 */
export function GenerativeUI({ iii }: { iii: any }) {
  const [spec, setSpec] = useState<Record<string, unknown> | null>(null);
  const [prompt, setPrompt] = useState("");
  const [loading, setLoading] = useState(false);
  const [refineText, setRefineText] = useState("");
  const sfRef = useRef<SpecForge | null>(null);

  useEffect(() => {
    const sf = createSpecForge(iii, {
      catalog,
      onPatch: (data) => {
        if (data.patch?.spec) setSpec(data.patch.spec);
      },
    });
    sfRef.current = sf;
    return () => { sf.shutdown(); };
  }, [iii]);

  const generate = useCallback(async () => {
    if (!sfRef.current) return;
    setLoading(true);
    try {
      const res = await sfRef.current.generate(prompt);
      if (res.spec) setSpec(res.spec);
    } finally {
      setLoading(false);
    }
  }, [prompt]);

  const refine = useCallback(async () => {
    if (!sfRef.current || !spec) return;
    setLoading(true);
    try {
      const res = await sfRef.current.refine(refineText, spec);
      if (res.spec) setSpec(res.spec);
      setRefineText("");
    } finally {
      setLoading(false);
    }
  }, [spec, refineText]);

  const streamGenerate = useCallback(async () => {
    if (!sfRef.current) return;
    setLoading(true);
    setSpec(null);
    try {
      await sfRef.current.stream(prompt);
    } finally {
      setLoading(false);
    }
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
