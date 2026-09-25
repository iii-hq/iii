import "server-only"

import { readFileSync } from "node:fs"
import { join } from "node:path"

export type ImageSize = { width: number; height: number }

const cache = new Map<string, ImageSize | null>()

/**
 * Pixel size of an image under `public/`, read from the file header so
 * `next/image` gets real dimensions and the layout never shifts. Handles PNG
 * and JPEG (several blog banners are JPEGs saved with a .png extension).
 * Returns null for anything else.
 */
export function publicImageSize(publicPath: string): ImageSize | null {
  const hit = cache.get(publicPath)
  if (hit !== undefined) return hit
  let size: ImageSize | null = null
  try {
    const buf = readFileSync(join(process.cwd(), "public", publicPath))
    size = pngSize(buf) ?? jpegSize(buf)
  } catch {
    size = null
  }
  cache.set(publicPath, size)
  return size
}

function pngSize(buf: Buffer): ImageSize | null {
  // 8-byte signature, then the IHDR chunk: length(4) type(4) width(4) height(4)
  if (buf.length < 24 || buf.toString("ascii", 1, 4) !== "PNG") return null
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) }
}

function jpegSize(buf: Buffer): ImageSize | null {
  if (buf.length < 4 || buf[0] !== 0xff || buf[1] !== 0xd8) return null
  let offset = 2
  while (offset + 9 < buf.length) {
    if (buf[offset] !== 0xff) return null
    const marker = buf[offset + 1]
    // Start-of-frame markers carry the dimensions (skip DHT, JPG, DAC).
    const isSof = marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc
    if (isSof) return { height: buf.readUInt16BE(offset + 5), width: buf.readUInt16BE(offset + 7) }
    offset += 2 + buf.readUInt16BE(offset + 2)
  }
  return null
}
