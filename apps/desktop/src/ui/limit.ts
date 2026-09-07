/**
 * A gate that lets `limit` calls run at once and queues the rest in order.
 *
 * The details panel mounts every photo of a conversation at the same moment,
 * and the thread mounts everything the virtual list holds, so anything those
 * do per picture (a download, an ffmpeg run) arrives as one burst. On a small
 * machine that burst is the whole problem: it is the same work either way,
 * just not all at once.
 */
export function slots(limit: number): <T>(run: () => Promise<T>) => Promise<T> {
  let active = 0
  const waiting: Array<() => void> = []
  return async <T,>(run: () => Promise<T>): Promise<T> => {
    if (active >= limit) await new Promise<void>((resolve) => waiting.push(resolve))
    active += 1
    try {
      return await run()
    } finally {
      active -= 1
      waiting.shift()?.()
    }
  }
}

/** Keyed de-duplication: the gallery and the thread ask for the same cut of the same photo. */
export function once<T>(): (key: string, run: () => Promise<T>) => Promise<T> {
  const inFlight = new Map<string, Promise<T>>()
  return (key, run) => {
    const pending = inFlight.get(key)
    if (pending) return pending
    const started = run().finally(() => inFlight.delete(key))
    inFlight.set(key, started)
    return started
  }
}
