// ─────────────────────────────────────────────────────────────
//
//  Browser Side: How your React app uses iii-render
//
//  NOTHING changes about json-render. You still use:
//    - defineCatalog() to define your components
//    - defineRegistry() to register React components
//    - <Renderer spec={spec} /> to render
//
//  The ONLY change: instead of calling Claude API directly from
//  the browser, you call your iii-render Rust server.
//
// ─────────────────────────────────────────────────────────────

import { defineCatalog } from "@json-render/core";
import { defineRegistry, Renderer } from "@json-render/react";
import { schema } from "@json-render/react/schema";
import { z } from "zod";
import { useState } from "react";

// ── Step 1: Same catalog as before (unchanged) ──────────────

const catalog = defineCatalog(schema, {
  components: {
    Card: {
      props: z.object({ title: z.string() }),
      description: "A card container",
    },
    Metric: {
      props: z.object({
        label: z.string(),
        value: z.string(),
        format: z.enum(["currency", "percent", "number"]).nullable(),
      }),
      description: "Display a metric value",
    },
    Button: {
      props: z.object({ label: z.string(), action: z.string() }),
      description: "Clickable button",
    },
  },
  actions: {
    export_report: { description: "Export dashboard to PDF" },
  },
});

// ── Step 2: Same registry as before (unchanged) ─────────────

const { registry } = defineRegistry(catalog, {
  components: {
    Card: ({ props, children }) => (
      <div className="rounded-lg border p-4">
        <h3 className="text-lg font-bold">{props.title}</h3>
        {children}
      </div>
    ),
    Metric: ({ props }) => (
      <div className="flex justify-between py-2">
        <span className="text-gray-500">{props.label}</span>
        <span className="font-mono font-bold">{props.value}</span>
      </div>
    ),
    Button: ({ props, emit }) => (
      <button
        className="mt-2 rounded bg-blue-500 px-4 py-2 text-white"
        onClick={() => emit("press")}
      >
        {props.label}
      </button>
    ),
  },
});

// ── Step 3: NEW — Call iii-render instead of Claude directly ─

export function Dashboard() {
  const [spec, setSpec] = useState(null);
  const [loading, setLoading] = useState(false);

  // ── Option A: Non-streaming (simple) ──────────────────────
  //
  // Full spec arrives at once. Simpler but user waits 1-5 seconds.

  async function generateSimple(prompt: string) {
    setLoading(true);
    const res = await fetch("http://localhost:3111/generate", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        prompt,
        catalog: {
          components: {
            Card: { description: "A card container", props: {} },
            Metric: {
              description: "Display a metric",
              props: { label: "string", value: "string" },
            },
            Button: {
              description: "Clickable button",
              props: { label: "string" },
            },
          },
          actions: {
            export_report: { description: "Export to PDF" },
          },
        },
      }),
    });
    const data = await res.json();
    setSpec(data.spec); // <── this is the json-render spec
    setLoading(false);
    console.log(`Generated in ${data.generation_ms}ms, cached: ${data.cached}`);
  }

  // ── Option B: Streaming (progressive rendering) ───────────
  //
  // Elements arrive one by one. UI builds in real-time.
  // THIS is the big UX win.

  async function generateStreaming(prompt: string) {
    setLoading(true);
    const res = await fetch("http://localhost:3111/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ prompt, catalog: {}, stream: true }),
    });

    const reader = res.body!.getReader();
    const decoder = new TextDecoder();

    // Build the spec incrementally
    let partialSpec = { root: "", elements: {} };

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      const text = decoder.decode(value);
      for (const line of text.split("\n")) {
        if (!line.startsWith("data: ")) continue;
        const event = JSON.parse(line.slice(6));

        if (event.root) {
          // First event: we know the root element ID
          partialSpec.root = event.root;
        }

        if (event.element) {
          // New element arrived — add it and re-render
          partialSpec.elements[event.id] = event.element;
          setSpec({ ...partialSpec }); // triggers React re-render
          // User sees the UI building piece by piece!
        }

        if (event.done) {
          // Final validated spec
          setSpec(event.spec);
          setLoading(false);
        }
      }
    }
  }

  // ── Render ────────────────────────────────────────────────

  return (
    <div>
      <button onClick={() => generateStreaming("Show me a sales dashboard with revenue, growth rate, and an export button")}>
        Generate Dashboard
      </button>

      {loading && <p>Generating...</p>}

      {spec && <Renderer spec={spec} registry={registry} />}
      {/*       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
                This is 100% standard json-render.
                No changes to the rendering layer at all.       */}
    </div>
  );
}
