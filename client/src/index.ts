export interface ComponentDef {
  description: string;
  props?: Record<string, unknown>;
  children?: boolean;
}

export interface ActionDef {
  description: string;
}

export interface Catalog {
  components: Record<string, ComponentDef>;
  actions?: Record<string, ActionDef>;
}

export interface UIElement {
  type: string;
  props: Record<string, unknown>;
  children: string[];
}

export interface UISpec {
  root: string;
  elements: Record<string, UIElement>;
}

export interface GenerateResponse {
  spec: UISpec;
  cached: boolean;
  generation_ms: number;
  model: string;
}

export interface RefineResponse {
  spec: UISpec;
  patches: SpecPatch[];
  patch_count: number;
  generation_ms: number;
  model: string;
}

export interface SpecPatch {
  op: "add" | "replace" | "remove" | "set_root";
  id?: string;
  element?: UIElement;
  root?: string;
}

export interface ValidationResult {
  valid: boolean;
  errors: string[];
}

export interface Stats {
  rate_limiter: {
    total_processed: number;
    total_rejected: number;
    current_pending: number;
    avg_wait_us: number;
  };
  cache: {
    exact_entries: number;
  };
}

export interface StreamEvent {
  type: "root" | "element" | "done" | "error";
  root?: string;
  id?: string;
  element?: UIElement;
  spec?: UISpec;
  cached?: boolean;
  errors?: string[];
  error?: string;
}

export interface ClientOptions {
  baseUrl?: string;
  model?: string;
}

export class IIIRenderClient {
  private baseUrl: string;
  private model: string;

  constructor(opts: ClientOptions = {}) {
    this.baseUrl = (opts.baseUrl ?? "http://localhost:3112").replace(/\/$/, "");
    this.model = opts.model ?? "claude-sonnet-4-20250514";
  }

  async generate(prompt: string, catalog: Catalog, model?: string): Promise<GenerateResponse> {
    const res = await fetch(`${this.baseUrl}/generate`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ prompt, catalog, model: model ?? this.model }),
    });
    if (!res.ok) throw new Error(`Generate failed: ${res.status} ${await res.text()}`);
    return res.json();
  }

  async *stream(
    prompt: string,
    catalog: Catalog,
    model?: string
  ): AsyncGenerator<StreamEvent, void, unknown> {
    const res = await fetch(`${this.baseUrl}/stream`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ prompt, catalog, model: model ?? this.model }),
    });
    if (!res.ok) throw new Error(`Stream failed: ${res.status}`);

    const reader = res.body!.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() ?? "";

        let currentEvent = "";
        for (const line of lines) {
          if (line.startsWith("event: ")) {
            currentEvent = line.slice(7).trim();
          } else if (line.startsWith("data: ")) {
            const data = JSON.parse(line.slice(6));
            yield { type: currentEvent as StreamEvent["type"], ...data };
          }
        }
      }
    } finally {
      reader.releaseLock();
    }
  }

  async refine(
    prompt: string,
    currentSpec: UISpec,
    catalog: Catalog,
    model?: string
  ): Promise<RefineResponse> {
    const res = await fetch(`${this.baseUrl}/refine`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        prompt,
        current_spec: currentSpec,
        catalog,
        model: model ?? this.model,
      }),
    });
    if (!res.ok) throw new Error(`Refine failed: ${res.status} ${await res.text()}`);
    return res.json();
  }

  async validate(spec: UISpec, catalog: Catalog): Promise<ValidationResult> {
    const res = await fetch(`${this.baseUrl}/validate`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ spec, catalog }),
    });
    if (!res.ok) throw new Error(`Validate failed: ${res.status}`);
    return res.json();
  }

  async stats(): Promise<Stats> {
    const res = await fetch(`${this.baseUrl}/stats`);
    if (!res.ok) throw new Error(`Stats failed: ${res.status}`);
    return res.json();
  }

  async health(): Promise<boolean> {
    try {
      const res = await fetch(`${this.baseUrl}/health`);
      return res.ok;
    } catch {
      return false;
    }
  }
}

export function renderSpec(spec: UISpec, container: HTMLElement): void {
  container.innerHTML = "";
  const rendered = renderElement(spec, spec.root);
  if (rendered) container.appendChild(rendered);
}

function renderElement(spec: UISpec, id: string): HTMLElement | null {
  const el = spec.elements[id];
  if (!el) return null;

  const node = document.createElement("div");
  node.dataset.iiiId = id;
  node.dataset.iiiType = el.type;

  switch (el.type) {
    case "Card":
      node.className = "iii-card";
      if (el.props.title) {
        const h = document.createElement("h3");
        h.textContent = String(el.props.title);
        node.appendChild(h);
      }
      break;

    case "Button": {
      const btn = document.createElement("button");
      btn.className = "iii-button";
      btn.textContent = String(el.props.label ?? "Button");
      if (el.props.variant === "primary") btn.classList.add("iii-primary");
      if (el.props.variant === "secondary") btn.classList.add("iii-secondary");
      if (el.props.variant === "danger") btn.classList.add("iii-danger");
      node.appendChild(btn);
      break;
    }

    case "Input": {
      const input = document.createElement("input");
      input.className = "iii-input";
      if (el.props.placeholder) input.placeholder = String(el.props.placeholder);
      if (el.props.type) input.type = String(el.props.type);
      node.appendChild(input);
      break;
    }

    case "Text": {
      const p = document.createElement("p");
      p.className = "iii-text";
      p.textContent = String(el.props.content ?? el.props.text ?? "");
      node.appendChild(p);
      break;
    }

    case "Link": {
      const a = document.createElement("a");
      a.className = "iii-link";
      a.textContent = String(el.props.text ?? el.props.label ?? "Link");
      a.href = String(el.props.href ?? "#");
      node.appendChild(a);
      break;
    }

    case "Image": {
      const img = document.createElement("img");
      img.className = "iii-image";
      if (el.props.src) img.src = String(el.props.src);
      if (el.props.alt) img.alt = String(el.props.alt);
      node.appendChild(img);
      break;
    }

    case "Metric": {
      const metric = document.createElement("div");
      metric.className = "iii-metric";
      if (el.props.label) {
        const label = document.createElement("span");
        label.className = "iii-metric-label";
        label.textContent = String(el.props.label);
        metric.appendChild(label);
      }
      const value = document.createElement("span");
      value.className = "iii-metric-value";
      value.textContent = String(el.props.value ?? "");
      metric.appendChild(value);
      node.appendChild(metric);
      break;
    }

    case "List": {
      const ul = document.createElement("ul");
      ul.className = "iii-list";
      node.appendChild(ul);
      break;
    }

    case "Table": {
      const table = document.createElement("table");
      table.className = "iii-table";
      if (Array.isArray(el.props.columns)) {
        const thead = document.createElement("thead");
        const tr = document.createElement("tr");
        for (const col of el.props.columns as string[]) {
          const th = document.createElement("th");
          th.textContent = col;
          tr.appendChild(th);
        }
        thead.appendChild(tr);
        table.appendChild(thead);
      }
      node.appendChild(table);
      break;
    }

    default: {
      node.className = "iii-unknown";
      const label = document.createElement("span");
      label.textContent = `[${el.type}]`;
      label.className = "iii-type-label";
      node.appendChild(label);
      if (el.props) {
        const pre = document.createElement("pre");
        pre.textContent = JSON.stringify(el.props, null, 2);
        node.appendChild(pre);
      }
    }
  }

  for (const childId of el.children) {
    const child = renderElement(spec, childId);
    if (child) node.appendChild(child);
  }

  return node;
}
