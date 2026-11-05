/**
 * Screenshots in the iii app section, one entry per device slot. The files are placeholders lifted from the app
 * repository's evidence (iii-hq/app `projects/*`) and the workers' store screenshots, converted to WebP at about
 * twice the size they are drawn (the static export serves them as they are, `images.unoptimized`). To replace one,
 * drop the new file in `public/iii-app/` and change `src`, `width`, `height` and `alt` here; the device frames crop
 * to their own shape, so a capture with a slightly different ratio still fits.
 */

export type AppShotSlot = 'mac' | 'iphone' | 'android'

export type AppShot = {
  src: string
  width: number
  height: number
  /** Says what the screen shows and on which device, since the frame around it is decorative. */
  alt: string
}

export const appShots: Record<AppShotSlot, AppShot> = {
  /* iii-hq/app projects/mac-app-launch/screenshots/: mac-home (2880 × 1896, a 1440 × 948 pt window), resized */
  mac: {
    src: '/iii-app/mac-home.webp',
    width: 1280,
    height: 843,
    alt: "The iii app on a Mac: an engine's Home, with one approval waiting in Chat, the apps shelf and the Chat, tmux, recent folders, connections, buckets and engine widgets.",
  },
  /* iii-hq/app projects/mac-app-launch/screenshots/: iphone-tmux-transcript (1320 × 2868), resized */
  iphone: {
    src: '/iii-app/iphone-tmux-subagent.webp',
    width: 440,
    height: 956,
    alt: 'The tmux app on an iPhone: the live transcript of a Claude Code sub-agent, with its Grep and Read calls and their results.',
  },
  /* iii-hq/workers ide/screenshots/android-editor-terminal.webp */
  android: {
    src: '/iii-app/android-ide.webp',
    width: 440,
    height: 988,
    alt: 'The IDE app on an Android phone: a TypeScript file in the editor above a terminal where the tests pass.',
  },
}
