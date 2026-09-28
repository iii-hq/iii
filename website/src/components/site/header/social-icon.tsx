import type { SVGProps } from "react"
import { DiscordIcon, GitHubIcon, LinkedInIcon, XIcon } from "@/components/site/icons"
import type { SocialId } from "./nav-links"

const icons = { github: GitHubIcon, discord: DiscordIcon, x: XIcon, linkedin: LinkedInIcon }

export function SocialIcon({ id, ...props }: { id: SocialId } & SVGProps<SVGSVGElement>) {
  const Icon = icons[id]
  return <Icon {...props} />
}
