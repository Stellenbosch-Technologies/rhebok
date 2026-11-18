import type { EngineInterface, Register } from 'claude-code'

const PLUGIN_ROOT = '${CLAUDE_PLUGIN_ROOT}'
const BIN = '${CLAUDE_PLUGIN_ROOT}/scripts/rhebok.sh'
const TIMEOUT_MS = 4000
const SLOW_MS = 1000
const TOOL = 'rhebok_context'
const SERVER = 'rhebok'
const COMMAND = 'rhebok-prehook'
const SETTINGS = ['mod', 'command', 'off'] as const

type Setting = (typeof SETTINGS)[number]

type StatusStyle = 'counts' | 'glyph' | 'words'

type Hit = { uri: string; title: string; relevance: number }

type Context = {
  block: string | null
  hits: Hit[]
  notes: string[]
  skipped: string | null
  elapsed_ms: number
  injector?: Setting
  status_style?: StatusStyle
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

function hitCount(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`
}

function firstNote(context: Context): string | undefined {
  return context.notes[0]?.replace(/^rhebok: /, '')
}

function counts(context: Context | undefined): string {
  if (context === undefined) return 'unavailable'
  if (context.injector !== undefined && context.injector !== 'mod') return `prehook ${context.injector}`
  const parts: string[] = []
  if (context.project) parts.push(context.project)
  const note = firstNote(context)
  if (note !== undefined) parts.push(note)
  else if (context.skipped !== null) parts.push('skipped')
  else if (context.block === null) parts.push(context.project ? 'no match' : 'nothing above the gate')
  else parts.push(hitCount(context.hits.length, 'hit', 'hits'), `${context.elapsed_ms} ms`)
  if (context.untrusted_project_file) parts.push('untrusted project file')
  return parts.join(' · ')
}

function glyphOutcome(context: Context): [string, string[]] {
  const note = firstNote(context)
  if (note !== undefined) return ['▲', [note]]
  if (context.skipped !== null) return ['○', ['skipped']]
  if (context.block === null) return ['○', ['no match']]
  return ['●', [String(context.hits.length), `${context.elapsed_ms} ms`]]
}

function glyph(context: Context | undefined): string {
  if (context === undefined) return '▲ unavailable'
  if (context.injector !== undefined && context.injector !== 'mod') return `– prehook ${context.injector}`
  const [mark, outcome] = glyphOutcome(context)
  const parts = [...(context.project ? [context.project] : []), ...outcome]
  if (context.untrusted_project_file) parts.push('untrusted project file')
  return `${mark} ${parts.join(' · ')}`
}

function words(context: Context | undefined): string {
  if (context === undefined) return 'not available'
  if (context.injector === 'command') return 'the command hook adds context'
  if (context.injector === 'off') return 'adding context is off'
  const from = context.project ? ` from ${context.project}` : ''
  const note = firstNote(context)
  let line: string
  if (note !== undefined) line = note
  else if (context.skipped !== null) line = 'not searched'
  else if (context.block === null) line = `nothing relevant${context.project ? ` in ${context.project}` : ''}`
  else {
    line = `added ${hitCount(context.hits.length, 'note', 'notes')}${from}`
    if (context.elapsed_ms >= SLOW_MS) line += ` · slow, ${(context.elapsed_ms / 1000).toFixed(1)} s`
  }
  return context.untrusted_project_file ? `${line} · project file not trusted` : line
}

export function statusFor(context: Context | undefined): string {
  switch (context?.status_style) {
    case 'glyph':
      return glyph(context)
    case 'words':
      return words(context)
    default:
      return counts(context)
  }
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
    $.ui.status(`prehook ${wanted}`)
    return { text: `rhebok prehook injector is now ${wanted}, in every session.` }
  })
}
