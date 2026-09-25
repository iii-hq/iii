import Image from "next/image"
import { publicImageSize } from "@/lib/image-size"
import { cn } from "@/lib/utils"

type PostImageProps = {
  /** public URL, e.g. `/blog/<slug>/banner.png` */
  src: string
  alt: string
  /** the `sizes` hint for the responsive srcset */
  sizes: string
  priority?: boolean
  className?: string
}

/**
 * A blog banner with the prose-image treatment (12px radius, hairline outline
 * that flips with the theme). Dimensions come from the file header so the
 * box is reserved before the image loads.
 */
export function PostImage({ src, alt, sizes, priority, className }: PostImageProps) {
  const size = publicImageSize(src)
  if (!size) return null
  return (
    <Image
      src={src}
      alt={alt}
      width={size.width}
      height={size.height}
      sizes={sizes}
      priority={priority}
      className={cn("h-auto w-full rounded-[12px] outline -outline-offset-1 outline-line", className)}
    />
  )
}
