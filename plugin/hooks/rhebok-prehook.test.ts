import type { On } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { TIMED_OUT, binaryFor, blockToAttach, resolveContext, statusFor } from './rhebok-prehook'

const BLOCK = '<rhebok-context>\npooling note\n</rhebok-context>'

function answer(injector: 'mod' | 'command' | 'off', block: string | null) {
  return {
    block,
    hits: block ? [{ uri: 'file:///n/pooling.md', title: 'Pooling', relevance: 0.81 }] : [],
    notes: [],
    skipped: injector === 'mod' ? null : `\`[prehook] injector\` is ${injector}`,
    elapsed_ms: 42,
    injector,
  }
}

describe('statusFor', () => {
  test('counts hits and time', () => {
    expect(statusFor(answer('mod', BLOCK))).toBe('1 hit · 42 ms')
  })
  test('says when another injector is set', () => {
    expect(statusFor(answer('command', null))).toBe('prehook command')
  })
  test('says when nothing cleared the gate', () => {
    expect(statusFor(answer('mod', null))).toBe('nothing above the gate')
  })
  test('shows the first note without its prefix', () => {
    const degraded = { ...answer('mod', BLOCK), notes: ['rhebok: retrieval degraded — budget'] }
    expect(statusFor(degraded)).toBe('retrieval degraded — budget')
  })
  test('names the project', () => {
    expect(statusFor({ ...answer('mod', BLOCK), project: 'api' })).toBe('api · 1 hit · 42 ms')
  })
  test('says a project had no match rather than that something broke', () => {
    expect(statusFor({ ...answer('mod', null), project: 'api' })).toBe('api · no match')
  })
  test('says when a repo project file is waiting to be trusted', () => {
    const waiting = { ...answer('mod', BLOCK), untrusted_project_file: '/r/.rhebok/project.toml' }
    expect(statusFor(waiting)).toBe('1 hit · 42 ms · untrusted project file')
  })
  test('says when rhebok could not be reached', () => {
    expect(statusFor(undefined)).toBe('unavailable')
  })
})

describe('statusFor with the glyph style', () => {
  const glyph = (c: ReturnType<typeof answer>) => ({ ...c, status_style: 'glyph' as const })
  test('marks an injection with a filled dot', () => {
    expect(statusFor(glyph(answer('mod', BLOCK)))).toBe('● 1 · 42 ms')
  })
  test('marks no match with an open dot and keeps the project', () => {
    expect(statusFor({ ...glyph(answer('mod', null)), project: 'api' })).toBe('○ api · no match')
  })
  test('marks a note as a warning', () => {
    const degraded = { ...glyph(answer('mod', BLOCK)), notes: ['rhebok: retrieval degraded — budget'] }
    expect(statusFor(degraded)).toBe('▲ retrieval degraded — budget')
  })
  test('marks another injector with a dash', () => {
    expect(statusFor(glyph(answer('off', null)))).toBe('– prehook off')
  })
})

describe('statusFor with the words style', () => {
  const words = (c: ReturnType<typeof answer>) => ({ ...c, status_style: 'words' as const })
  test('says how many notes it added and from where', () => {
    expect(statusFor({ ...words(answer('mod', BLOCK)), project: 'api' })).toBe('added 1 note from api')
  })
  test('leaves the time out until it is slow', () => {
    expect(statusFor({ ...words(answer('mod', BLOCK)), elapsed_ms: 999 })).toBe('added 1 note')
    expect(statusFor({ ...words(answer('mod', BLOCK)), elapsed_ms: 1250 })).toBe('added 1 note · slow, 1.3 s')
  })
  test('says nothing was relevant rather than naming the gate', () => {
    expect(statusFor(words(answer('mod', null)))).toBe('nothing relevant')
  })
  test('says who adds context when the mod does not', () => {
    expect(statusFor(words(answer('command', null)))).toBe('the command hook adds context')
    expect(statusFor(words(answer('off', null)))).toBe('adding context is off')
  })
})

describe('statusFor never repeats the name the host already shows', () => {
  test('in any style or state', () => {
    for (const status_style of ['counts', 'glyph', 'words'] as const) {
      for (const c of [answer('mod', BLOCK), answer('mod', null), answer('command', null), undefined]) {
        const shown = statusFor(c === undefined ? undefined : { ...c, status_style })
        expect(shown.startsWith('rhebok')).toBe(false)
      }
    }
  })
})

describe('resolveContext', () => {
  test('uses the server when it answers', async () => {
    let commandRan = false
    const got = await resolveContext({
      server: async () => answer('mod', BLOCK),
      command: async () => {
        commandRan = true
        return undefined
      },
    })
    expect(got?.block).toBe(BLOCK)
    expect(commandRan).toBe(false)
  })
  test('falls back to the command when there is no server', async () => {
    const got = await resolveContext({ command: async () => answer('mod', BLOCK) })
    expect(got?.block).toBe(BLOCK)
  })
  test('falls back to the command when the server fails', async () => {
    const got = await resolveContext({
      server: async () => undefined,
      command: async () => answer('mod', BLOCK),
    })
    expect(got?.block).toBe(BLOCK)
  })
  test('does not stack a second wait on a server that timed out', async () => {
    let commandRan = false
    const got = await resolveContext({
      server: async () => TIMED_OUT,
      command: async () => {
        commandRan = true
        return answer('mod', BLOCK)
      },
    })
    expect(got).toBeUndefined()
    expect(commandRan).toBe(false)
  })
})

describe('blockToAttach', () => {
  test('attaches the block when the mod is the injector', () => {
    expect(blockToAttach(answer('mod', BLOCK))).toBe(BLOCK)
  })
  test('attaches nothing when the command hook is the injector', () => {
    expect(blockToAttach({ ...answer('command', null), block: BLOCK })).toBeUndefined()
  })
  test('attaches nothing when nothing cleared the gate', () => {
    expect(blockToAttach(answer('mod', null))).toBeUndefined()
  })
  test('attaches nothing when rhebok was unreachable', () => {
    expect(blockToAttach(undefined)).toBeUndefined()
  })
})

describe('binaryFor', () => {
  const launcher = ['$', '{CLAUDE_PLUGIN_ROOT}', '/scripts/launch'].join('')
  test('runs the launcher inside the plugin a marketplace installed', () => {
    expect(binaryFor(launcher, () => '/cache/rhebok/0.1.0')).toBe('/cache/rhebok/0.1.0/scripts/launch')
  })
  test('runs the binary rhebok install wrote in, without asking for the plugin root', () => {
    let asked = false
    const root = () => {
      asked = true
      return '/cache/rhebok/0.1.0'
    }
    expect(binaryFor('/opt/rhebok/bin/rhebok', root)).toBe('/opt/rhebok/bin/rhebok')
    expect(asked).toBe(false)
  })
})

const LAUNCHER = /^\/.+\/scripts\/rhebok\.sh$/

function recordRuns(on: On, stdout: string): string[][] {
  const ran: string[][] = []
  on('mcp.connect', () => ({ value: { isConnected: false, reason: 'unlisted', message: 'no rhebok server' } }))
  on('ui.status', () => ({ value: undefined }))
  on('process.run', (_, e) => {
    ran.push([...e.argv])
    return { value: { exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  return ran
}

const PRESENTATION = { isFullscreen: false, columns: 80 }

describe('a marketplace install runs the launcher inside its own plugin folder', () => {
  test('at session start', async ($, on) => {
    const ran = recordRuns(on, '{}')
    on('session.start', () => ({ cwd: '/' }))
    on('session.id', () => ({ value: 'sess-1' }))
    on('command.register', () => ({ value: { command: 'rhebok-prehook' } }))
    await $.session.start({ cwd: '/', surface: 'terminal', isInteractive: true })
    expect(ran.length).toBe(1)
    expect(ran[0]?.[0]).toMatch(LAUNCHER)
    expect(ran[0]?.slice(1)).toEqual(['hook', 'mod-start'])
  })
  test('when the prompt falls back to the command', async ($, on) => {
    const ran = recordRuns(on, JSON.stringify(answer('mod', BLOCK)))
    let attached: readonly string[] | undefined
    on('prompt.submit', (_, e) => {
      attached = e.context
      return { text: e.text }
    })
    await $.prompt.submit({ text: 'how do we pool', wait: false, origin: { kind: 'composer' } })
    expect(ran.length).toBe(1)
    expect(ran[0]?.[0]).toMatch(LAUNCHER)
    expect(ran[0]?.slice(1, 4)).toEqual(['context', '--json', '--prehook'])
    expect(attached).toEqual([BLOCK])
  })
  test('when /rhebok-prehook reads the setting', async ($, on) => {
    const ran = recordRuns(on, 'command\n')
    const shown = await $.command.run({
      command: 'rhebok-prehook',
      args: '',
      origin: { kind: 'composer' },
      presentation: PRESENTATION,
    })
    expect(ran.length).toBe(1)
    expect(ran[0]?.[0]).toMatch(LAUNCHER)
    expect(ran[0]?.slice(1)).toEqual(['config', 'get', 'prehook.injector'])
    expect(shown.text).toContain('rhebok prehook injector: command')
  })
  test('when /rhebok-prehook switches the setting', async ($, on) => {
    const ran = recordRuns(on, '')
    await $.command.run({
      command: 'rhebok-prehook',
      args: 'mod',
      origin: { kind: 'composer' },
      presentation: PRESENTATION,
    })
    expect(ran.length).toBe(2)
    expect(ran[0]?.[0]).toMatch(LAUNCHER)
    expect(ran[0]?.slice(1)).toEqual(['config', 'set', 'prehook.injector', 'mod'])
    expect(ran[1]?.[0]).toMatch(LAUNCHER)
    expect(ran[1]?.slice(1)).toEqual(['hook', 'mod-start'])
  })
})
