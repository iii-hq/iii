/**
 * The hero's background film, the way trigger.dev does it: a recording of our own console stage (`/a/stage`),
 * pre-blurred and darkened at encode time so the browser does no filter work, tilted into perspective and veiled
 * so the headline stays the brightest thing on the page. Muted, looping, no controls. With reduced motion the
 * poster frame stands in for the video.
 */
export function HeroBackdrop() {
  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 overflow-hidden [perspective:1400px]">
      <div className="absolute top-1/2 left-1/2 h-[160%] w-[175%] [transform:translate(-50%,-50%)_rotateX(44deg)_rotateY(-14deg)_rotateZ(9deg)] [transform-style:preserve-3d]">
        <video
          className="h-full w-full object-cover motion-reduce:hidden"
          autoPlay
          muted
          loop
          playsInline
          disablePictureInPicture
          preload="metadata"
          poster="/hero/system-poster.jpg"
        >
          <source src="/hero/system.webm" type="video/webm" />
          <source src="/hero/system.mp4" type="video/mp4" />
        </video>
        {/* biome-ignore lint/performance/noImgElement: a decorative fallback for reduced motion, same file as the poster. */}
        <img src="/hero/system-poster.jpg" alt="" className="hidden h-full w-full object-cover motion-reduce:block" />
      </div>
      {/* The veil: a flat dim, a vignette that hides the film's edges, and a fade into the page below. */}
      <div className="absolute inset-0 bg-background/25" />
      <div className="absolute inset-0 bg-[radial-gradient(80%_70%_at_50%_42%,transparent_35%,var(--background)_100%)]" />
      <div className="absolute inset-x-0 bottom-0 h-48 bg-linear-to-b from-transparent to-background" />
    </div>
  )
}
