import type { EngineInterface, Register } from 'claude-code'

const PLUGIN_ROOT = '${CLAUDE_PLUGIN_ROOT}'
const BIN = '${CLAUDE_PLUGIN_ROOT}/scripts/rhebok.sh'
const TIMEOUT_MS = 4000
const TOOL = 'rhebok_context'
const SERVER = 'rhebok'
const COMMAND = 'rhebok-prehook'
const SETTINGS = ['mod', 'command', 'off'] as const

type Setting = (typeof SETTINGS)[number]

type Hit = { uri: string; title: string; relevance: number }

type Context = {
  block: string | null
  hits: Hit[]
  notes: string[]
  skipped: string | null
  elapsed_ms: number
  injector?: Setting
  project?: string | null
  untrusted_project_file?: string | null
}

const TIMED_OUT: unique symbol = Symbol('timed out')

export function binaryFor(bin: string, pluginRoot: () => string): string {
  return bin.startsWith(PLUGIN_ROOT) ? pluginRoot() + bin.slice(PLUGIN_ROOT.length) : bin
}

function rhebok($: EngineInterface): string {
  return binaryFor(BIN, () => $.plugin.root)
}

function outcome(context: Context): string {
  const note = context.notes[0]
  if (note !== undefined) return note.replace(/^rhebok: /, '')
  if (context.skipped !== null) return 'skipped'
  if (context.block === null) {
    return context.project ? 'no match' : 'nothing above the gate'
  }
  const hits = context.hits.length
  return `${hits} ${hits === 1 ? 'hit' : 'hits'} · ${context.elapsed_ms} ms`
}

export function statusFor(context: Context | undefined): string {
  if (context === undefined) return 'rhebok · unavailable'
  if (context.injector !== undefined && context.injector !== 'mod') {
    return `rhebok · prehook ${context.injector}`
  }
  const parts = ['rhebok']
  if (context.project) parts.push(context.project)
  parts.push(outcome(context))
  if (context.untrusted_project_file) parts.push('untrusted project file')
  return parts.join(' · ')
}

async function connectServer($: EngineInterface): Promise<string | undefined> {
  const connected = await $.mcp.connect(SERVER)
  return connected.isConnected ? connected.server : undefined
}

function parse(text: string): Context | undefined {
  try {
    return JSON.parse(text) as Context
  } catch {
    return undefined
  }
}

async function viaServer(
  $: EngineInterface,
  server: string,
  args: Record<string, unknown>,
): Promise<Context | undefined | typeof TIMED_OUT> {
  const call = $.mcp
    .call(server, TOOL, args)
    .then(result => {
      const block = result.content.find(c => c.type === 'text')
      const text = block?.type === 'text' ? block.text : undefined
      return result.isError || typeof text !== 'string' ? undefined : parse(text)
    })
    .catch(() => undefined)
  const timeout = $.clock.sleep(TIMEOUT_MS).then((): typeof TIMED_OUT => TIMED_OUT)
  return Promise.race([call, timeout])
}

async function viaCommand($: EngineInterface, text: string, session: string, cwd: string) {
  const run = await $.process.run(
    [rhebok($), 'context', '--json', '--prehook', '--session', session, '--cwd', cwd],
    { stdin: text, timeoutMs: TIMEOUT_MS },
  )
  return run.exitCode === 0 ? parse(run.stdout) : undefined
}

export type Sources = {
  server?: () => Promise<Context | undefined | typeof TIMED_OUT>
  command: () => Promise<Context | undefined>
}

export async function resolveContext(sources: Sources): Promise<Context | undefined> {
  const answered = sources.server === undefined ? undefined : await sources.server()
  if (answered === TIMED_OUT) return undefined
  return answered ?? (await sources.command())
}

export function blockToAttach(context: Context | undefined): string | undefined {
  return context?.injector === 'mod' && context.block ? context.block : undefined
}

export { TIMED_OUT }

export const register: Register = on => {
  let server: string | undefined
  let session = ''
  let cwd = ''

  on('session.start', async ($, e, next) => {
    const started = await next(e)
    session = await $.session.id()
    cwd = e.cwd
    await $.process.run([rhebok($), 'hook', 'mod-start'], {
      stdin: JSON.stringify({ session_id: session }),
      timeoutMs: TIMEOUT_MS,
    })
    server = await connectServer($)
    await $.command.register({
      name: COMMAND,
      description: 'Show or switch who injects rhebok context: mod, command or off',
    })
    return started
  })

  on('prompt.submit', async ($, e, next) => {
    if (e.text.trimStart().startsWith('/')) return next(e)
    server = server ?? (await connectServer($))
    const args = { message: e.text, prehook: true, session, cwd }
    const named = server
    const context = await resolveContext({
      server: named === undefined ? undefined : () => viaServer($, named, args),
      command: () => viaCommand($, e.text, session, cwd),
    })
    $.ui.status(statusFor(context))
    const block = blockToAttach(context)
    return block === undefined ? next(e) : next({ ...e, context: [...(e.context ?? []), block] })
  }).catch(($, e, next) => next(e))

  on('command.run', { command: COMMAND }, async ($, e) => {
    const wanted = e.args.trim()
    if (wanted === '') {
      const run = await $.process.run([rhebok($), 'config', 'get', 'prehook.injector'])
      const current = run.stdout.trim() || 'mod'
      return { text: `rhebok prehook injector: ${current} (switch with /${COMMAND} mod|command|off)` }
    }
    if (!SETTINGS.includes(wanted as Setting)) {
      return { text: `Unknown setting \`${wanted}\`. Use one of: ${SETTINGS.join(', ')}.` }
    }
    const set = await $.process.run([rhebok($), 'config', 'set', 'prehook.injector', wanted])
    if (set.exitCode !== 0) return { text: `Could not switch: ${set.stderr.trim()}` }
    if (wanted === 'mod') {
      await $.process.run([rhebok($), 'hook', 'mod-start'], {
        stdin: JSON.stringify({ session_id: session }),
      })
    }
    $.ui.status(`rhebok · prehook ${wanted}`)
    return { text: `rhebok prehook injector is now ${wanted}, in every session.` }
  })
}
