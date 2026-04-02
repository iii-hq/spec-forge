import { createIIIStateStore, type IIIStateStoreExtended } from "./state-store"
import { createIIIActionRouter, type IIIActionRouter } from "./action-router"
import { resolveExpressions } from "./expressions"
import type { SpecForgeOptions, SpecForgeCatalog, PatchEvent, GenerateResult } from "./types"

export type { SpecForgeOptions, SpecForgeCatalog, PatchEvent, GenerateResult } from "./types"
export type { IIIStateStoreExtended } from "./state-store"
export type { IIIActionRouter } from "./action-router"
export { createIIIStateStore } from "./state-store"
export { createIIIActionRouter } from "./action-router"
export { resolveExpressions } from "./expressions"
export { createSessionManager } from "./session"
export * from "./types"

export interface SpecForge {
  generate(prompt: string, model?: string): Promise<GenerateResult>
  stream(prompt: string, model?: string): Promise<void>
  refine(prompt: string, currentSpec: Record<string, unknown>, model?: string): Promise<GenerateResult>
  validate(spec: Record<string, unknown>): Promise<{ valid: boolean; errors: string[] }>
  join(sessionId: string): Promise<void>
  leave(): Promise<void>
  stateStore: IIIStateStoreExtended
  actionRouter: IIIActionRouter
  shutdown(): Promise<void>
}

export function createSpecForge(iii: any, opts: SpecForgeOptions): SpecForge {
  const scope = opts.scope ?? "spec-forge"
  const catalog = opts.catalog
  const model = opts.model ?? "claude-sonnet-4-20250514"
  let sessionId: string | null = null
  let sessionTriggerRef: { unregister: () => void } | null = null

  const stateStore = createIIIStateStore(iii, scope)
  const actionRouter = createIIIActionRouter(iii)

  const refs: Array<{ unregister: () => void }> = []

  refs.push(
    iii.registerFunction({ id: "ui::render-patch" }, async (data: PatchEvent) => {
      opts.onPatch?.(data)
      return { applied: true }
    }),
  )

  refs.push(
    iii.registerFunction(
      { id: "ui::state-update" },
      async (data: { scope: string; key: string; value: unknown }) => {
        stateStore._applyRemoteUpdate(data.key, data.value)
        opts.onStateChange?.(data.key, data.value)
        return { received: true }
      },
    ),
  )

  refs.push(
    iii.registerFunction(
      { id: "ui::notification" },
      async (data: { type: string; payload: unknown }) => {
        opts.onNotification?.(data)
        return { displayed: true }
      },
    ),
  )

  refs.push(
    iii.registerFunction(
      { id: "ui::stream-update" },
      async (data: { stream: string; group: string; item: string; value: unknown }) => {
        stateStore._applyRemoteUpdate(
          `/__streams__/${data.stream}/${data.group}/${data.item}`,
          data.value,
        )
        return { updated: true }
      },
    ),
  )

  return {
    stateStore,
    actionRouter,

    async generate(prompt: string, mdl?: string) {
      return iii.trigger({
        function_id: "spec-forge::generate",
        payload: { prompt, catalog, model: mdl ?? model, session_id: sessionId },
      })
    },

    async stream(prompt: string, mdl?: string) {
      return iii.trigger({
        function_id: "spec-forge::stream",
        payload: { prompt, catalog, model: mdl ?? model, session_id: sessionId },
      })
    },

    async refine(prompt: string, currentSpec: Record<string, unknown>, mdl?: string) {
      return iii.trigger({
        function_id: "spec-forge::refine",
        payload: {
          prompt,
          current_spec: currentSpec,
          catalog,
          model: mdl ?? model,
          session_id: sessionId,
        },
      })
    },

    async validate(spec: Record<string, unknown>) {
      return iii.trigger({
        function_id: "spec-forge::validate",
        payload: { spec, catalog },
      })
    },

    async join(sid: string) {
      const triggerRef = iii.registerTrigger({
        type: "state",
        function_id: "ui::state-update",
        config: { scope: `session::${sid}` },
      })
      await iii.trigger({
        function_id: "spec-forge::join-session",
        payload: { session_id: sid },
      })
      sessionId = sid
      sessionTriggerRef = triggerRef
    },

    async leave() {
      if (!sessionId) return
      await iii.trigger({
        function_id: "spec-forge::leave-session",
        payload: { session_id: sessionId },
      })
      if (sessionTriggerRef) {
        sessionTriggerRef.unregister()
        sessionTriggerRef = null
      }
      sessionId = null
    },

    async shutdown() {
      if (sessionId) await this.leave()
      refs.forEach((r) => r.unregister())
      refs.length = 0
    },
  }
}
