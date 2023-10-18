import type { ServerResponse } from 'node:http'

import './node-polyfill-web-streams'
import { abortControllerFromNodeResponse } from './web/spec-extension/adapters/next-request'

export function isAbortError(e: any): e is Error & { name: 'AbortError' } {
  return e?.name === 'AbortError'
}

function createWriterFromResponse(
  res: ServerResponse,
  controller: AbortController
): WritableStream<Uint8Array> {
  let started = false

  // Create a promise that will resolve once the response has drained. See
  // https://nodejs.org/api/stream.html#stream_event_drain
  let drained = Promise.withResolvers<void>()
  res.on('drain', () => {
    drained.resolve()
  })

  // Create a promise that will resolve once the response has finished. See
  // https://nodejs.org/api/http.html#event-finish_1
  const finished = Promise.withResolvers<void>()
  res.once('close', () => {
    // If the finish event fires, it means we shouldn't block and wait for the
    // drain event.
    drained.resolve()
  })

  // Once the response finishes, resolve the promise.
  res.once('finish', () => {
    finished.resolve()
  })

  // Create a writable stream that will write to the response.
  return new WritableStream<Uint8Array>({
    write: async (chunk) => {
      // You'd think we'd want to use `start` instead of placing this in `write`
      // but this ensures that we don't actually flush the headers until we've
      // started writing chunks.
      if (!started) {
        started = true
        res.flushHeaders()
      }

      try {
        const ok = res.write(chunk)

        // Added by the `compression` middleware, this is a function that will
        // flush the partially-compressed response to the client.
        if ('flush' in res && typeof res.flush === 'function') {
          res.flush()
        }

        // If the write returns false, it means there's some backpressure, so
        // wait until it's streamed before continuing.
        if (!ok) {
          // Reset the drained promise so that we can wait for the next drain event.
          drained = Promise.withResolvers<void>()

          await drained.promise
        }
      } catch (err: any) {
        controller.abort(err)
        throw err
      }
    },
    close: () => {
      if (res.writableFinished) return

      res.end()
      return finished.promise
    },
  })
}

export async function pipeToNodeResponse(
  readable: ReadableStream<Uint8Array>,
  res: ServerResponse
) {
  try {
    // If the response has already errored, then just return now.
    const { errored, destroyed } = res
    if (errored || destroyed) return

    // Create a new AbortController so that we can abort the readable if the
    // client disconnects.
    const controller = abortControllerFromNodeResponse(res)

    const writer = createWriterFromResponse(res, controller)

    await readable.pipeTo(writer, { signal: controller.signal })
  } catch (err: any) {
    // If this isn't related to an abort error, re-throw it.
    if (!isAbortError(err)) {
      throw err
    }
  }
}
