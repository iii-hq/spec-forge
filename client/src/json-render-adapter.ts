import { IIIRenderClient, type Catalog, type UISpec, type StreamEvent } from "./index.js";

export interface JsonRenderAdapter {
  generate(prompt: string): Promise<UISpec>;
  stream(prompt: string, onElement: (id: string, element: unknown) => void): Promise<UISpec>;
  refine(prompt: string): Promise<UISpec>;
  getSpec(): UISpec | null;
}

export function createAdapter(
  catalog: Catalog,
  opts: { baseUrl?: string; model?: string } = {}
): JsonRenderAdapter {
  const client = new IIIRenderClient(opts);
  let currentSpec: UISpec | null = null;

  return {
    async generate(prompt: string): Promise<UISpec> {
      const res = await client.generate(prompt, catalog);
      currentSpec = res.spec;
      return res.spec;
    },

    async stream(
      prompt: string,
      onElement: (id: string, element: unknown) => void
    ): Promise<UISpec> {
      let finalSpec: UISpec | null = null;

      for await (const event of client.stream(prompt, catalog)) {
        if (event.type === "element" && event.id && event.element) {
          onElement(event.id, event.element);
        }
        if (event.type === "done" && event.spec) {
          finalSpec = event.spec;
        }
      }

      if (finalSpec) {
        currentSpec = finalSpec;
        return finalSpec;
      }
      throw new Error("Stream ended without producing a spec");
    },

    async refine(prompt: string): Promise<UISpec> {
      if (!currentSpec) throw new Error("No spec to refine — call generate() first");
      const res = await client.refine(prompt, currentSpec, catalog);
      currentSpec = res.spec;
      return res.spec;
    },

    getSpec(): UISpec | null {
      return currentSpec;
    },
  };
}
