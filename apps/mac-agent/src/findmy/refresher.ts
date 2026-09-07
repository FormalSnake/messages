/**
 * Find My's caches only move while FindMy.app is running, and only for about
 * five minutes after it launches: left hidden it then stops polling and the
 * files stop changing, which is how a panel opened on Monday ends up showing
 * Wednesday's location. Nothing else refreshes them. `Devices.data` is written
 * by the app alone, and findmylocateagent keeps only the friend whose push
 * arrived last.
 *
 * Launching the app is what starts a fresh burst (FindMySyncPlus reached the
 * same conclusion in FindMyRefresher.swift), so the agent watches the mtimes
 * and restarts it whenever they stop moving. A Find My someone is actually
 * using is refreshing on its own, so it is never restarted under them.
 */

import { statSync } from 'node:fs'
import { deviceSourcePaths, friendSourcePaths } from './index'

/** The name `open` wants, and the process name too. `-x` keeps the match exact: `pgrep -f` on the bundle path would also hit any shell holding that string in its arguments. */
const APP = 'FindMy'
/** A refreshing Find My rewrites something every 20 to 35 seconds, whether or not anyone moved. Longer than this and it has gone quiet. */
const STALE_MS = 45_000
/** Floor between restarts, so a Mac whose caches never move (offline, or nothing shared with it) is not bounced in a loop. A burst lasts longer than this anyway. */
const RESTART_FLOOR_MS = 180_000
const CHECK_MS = 15_000

export function findMyRunning(): boolean {
  try {
    return Bun.spawnSync(['pgrep', '-x', APP]).exitCode === 0
  } catch {
    return false
  }
}

/** `-g` leaves the front window alone and `-j` starts the app hidden, so nothing takes focus. Answers whether it had to launch. */
export function launchFindMy(): boolean {
  if (findMyRunning()) return false
  Bun.spawnSync(['open', '-g', '-j', '-a', APP])
  return true
}

/** SIGTERM rather than an `osascript` quit: sending an Apple event would need automation rights this agent has no way to ask for. */
function quitFindMy(): void {
  Bun.spawnSync(['pkill', '-x', APP])
}

function newestSourceAt(): number {
  let newest = 0
  for (const file of [...friendSourcePaths(), ...deviceSourcePaths()]) {
    try {
      newest = Math.max(newest, statSync(file).mtimeMs)
    } catch {
      // Not every candidate path exists on every Mac.
    }
  }
  return newest
}

let lastRestartAt = 0

/** The mtimes come first so a Mac that is refreshing normally costs five `stat` calls and starts no processes at all. */
async function refreshCycle(): Promise<void> {
  const newest = newestSourceAt()
  // Nothing on disk to keep fresh: Find My has never run here.
  if (newest === 0) return
  const now = Date.now()
  if (now - newest < STALE_MS || now - lastRestartAt < RESTART_FLOOR_MS) return
  lastRestartAt = now
  // Not running at all: the launch is itself the refresh.
  if (launchFindMy()) {
    console.log('mac-agent: Find My was not running, launched it')
    return
  }
  console.log('mac-agent: Find My stopped writing its caches, restarting it')
  quitFindMy()
  await Bun.sleep(2000)
  launchFindMy()
}

/** Keeps Find My running and writing. Checks every `everyMs`, and only acts when the caches have stalled. */
export function keepFindMyOpen(everyMs = CHECK_MS): { stop: () => void } {
  void refreshCycle()
  const timer = setInterval(() => void refreshCycle(), everyMs)
  return { stop: () => clearInterval(timer) }
}
