import { once, slots } from './limit'

/**
 * ffmpeg runs at once. Every one of them decodes a full photo, and a screen
 * of pictures asks for its cuts all at the same moment; on a four watt laptop
 * that took every core and left nothing to paint with.
 */
export const ffmpegSlot = slots(2)
/** One run per output file, however many components are waiting on it. */
export const ffmpegOnce = once<string | null>()
