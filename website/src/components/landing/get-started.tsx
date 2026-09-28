import { ArrowRightIcon } from "@/components/site/iconly"
import { GitHubIcon } from "@/components/site/icons"
import { TrackedLink } from "@/components/site/tracked-link"
import { buttonVariants } from "@/components/ui/button-variants"
import { links } from "@/lib/site"
import { cn } from "@/lib/utils"
import { DiscordButton } from "./get-started/discord-button"
import { InstallCommand } from "./get-started/install-command"
import { SubscribeForm } from "./get-started/subscribe-form"

// Inner padding of the card; the email strip below shares it so edges line up.
const PAD = "px-6 sm:px-10 lg:px-14"

/**
 * The closer, as one card: the pitch on the left, the install command and the
 * three places to go next on the right, and a quiet email strip along the
 * bottom. Everything a reader might do from here, in one place, one size each.
 */
export function GetStarted() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: kept from the old page so iii.dev/#footer links still land here
    <section id="footer" aria-labelledby="get-started-title" className="scroll-mt-[116px] py-20 sm:py-24">
      <div className="mx-auto max-w-[1200px] px-5 md:px-6">
        <div className="overflow-hidden rounded-[22px] bg-gray-2 shadow-panel">
          <div
            className={cn(
              "grid gap-10 py-10 sm:py-12 lg:py-14 xl:grid-cols-[minmax(0,1fr)_minmax(0,640px)] xl:items-center xl:gap-16",
              PAD,
            )}
          >
            {/* `data-llms` marks the prose scripts/generate-llms-agents.ts extracts. */}
            <div data-llms="intro" className="max-w-[520px]">
              <p className="text-[13px] font-medium tracking-[0.02em] text-gray-10">Get started</p>
              {/* biome-ignore lint/correctness/useUniqueElementIds: the section's aria-labelledby targets this id; it renders once */}
              <h2
                id="get-started-title"
                className="mt-3 text-[clamp(30px,3.8vw,44px)] leading-[1.08] font-medium tracking-[-0.035em] text-balance text-gray-12"
              >
                Install iii in one command.
              </h2>
              <p className="mt-4 max-w-[440px] text-[15px] leading-[1.6] text-pretty text-gray-11">
                One line installs the engine. Then read the docs, star the repo, or come say hi on Discord.
              </p>
            </div>

            <div className="flex min-w-0 flex-col gap-3">
              <InstallCommand />
              <div className="flex flex-col gap-3 sm:flex-row sm:flex-wrap">
                <TrackedLink
                  href={links.docs}
                  cta={{ cta_id: "docs", cta_location: "footer", cta_label: "read the docs" }}
                  className={cn(buttonVariants({ size: "lg" }), "group/cta")}
                >
                  Read the docs
                  <ArrowRightIcon className="size-4 transition-transform duration-200 ease-out group-hover/cta:translate-x-0.5" />
                </TrackedLink>
                <TrackedLink
                  href={links.github}
                  target="_blank"
                  rel="noopener noreferrer"
                  cta={{ cta_id: "github", cta_location: "footer", cta_label: "github" }}
                  className={buttonVariants({ variant: "outline", size: "lg" })}
                >
                  <GitHubIcon className="size-4" />
                  GitHub
                </TrackedLink>
                <DiscordButton size="lg" />
              </div>
            </div>
          </div>

          <div
            className={cn(
              "flex flex-col gap-4 border-t border-gray-5 py-6 sm:flex-row sm:items-center sm:justify-between sm:gap-8",
              PAD,
            )}
          >
            <div>
              <p className="text-[14px] font-medium text-gray-12">Follow development</p>
              <p className="mt-0.5 text-[13px] leading-[1.6] text-gray-10">New specs and releases, by email.</p>
            </div>
            <SubscribeForm location="footer" className="w-full sm:w-[400px] sm:shrink-0" />
          </div>
        </div>
      </div>
    </section>
  )
}
