// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef } from 'react'
import { uploadConsoleSessionReplay } from '../api/platform'

function pickRecorderMime(): string | undefined {
  if (typeof MediaRecorder === 'undefined') return undefined
  for (const mime of ['video/webm;codecs=vp9', 'video/webm;codecs=vp8', 'video/webm']) {
    if (MediaRecorder.isTypeSupported(mime)) return mime
  }
  return undefined
}

type Options = {
  canvas: HTMLCanvasElement | null
  sessionId?: string | null
  recordingActive: boolean
  readOnly: boolean
}

/** Capture VNC canvas to `.webm` when enterprise recording is active; upload on stop. */
export function useConsoleSessionRecorder({ canvas, sessionId, recordingActive, readOnly }: Options) {
  const recorderRef = useRef<MediaRecorder | null>(null)
  const chunksRef = useRef<Blob[]>([])
  const sessionIdRef = useRef(sessionId)
  sessionIdRef.current = sessionId

  useEffect(() => {
    if (!canvas || !sessionId || !recordingActive || readOnly) return
    const mime = pickRecorderMime()
    if (!mime) return

    chunksRef.current = []
    const stream = canvas.captureStream(2)
    const recorder = new MediaRecorder(stream, { mimeType: mime })
    recorderRef.current = recorder

    recorder.ondataavailable = (ev) => {
      if (ev.data.size > 0) chunksRef.current.push(ev.data)
    }

    recorder.start(5000)

    return () => {
      recorderRef.current = null
      const sid = sessionIdRef.current
      const finalize = async () => {
        if (recorder.state !== 'inactive') {
          await new Promise<void>((resolve) => {
            recorder.addEventListener('stop', () => resolve(), { once: true })
            recorder.stop()
          })
        }
        stream.getTracks().forEach((t) => t.stop())
        if (!sid || chunksRef.current.length === 0) return
        const blob = new Blob(chunksRef.current, { type: mime.split(';')[0] })
        if (blob.size === 0) return
        try {
          // Ending the session itself (regardless of recording) is handled
          // by MachineCockpit's session-lifecycle effect — this hook is only
          // responsible for capturing and uploading the replay.
          await uploadConsoleSessionReplay(sid, blob)
        } catch {
          /* best-effort audit capture */
        }
      }
      void finalize()
    }
  }, [canvas, sessionId, recordingActive, readOnly])
}
