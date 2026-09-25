import { AGENT_MARKS } from "@/components/landing/hero/agent-marks"
import { TrackedLink } from "@/components/site/tracked-link"
import { buttonVariants } from "@/components/ui/button-variants"
import { ASK_AI_ASSISTANTS, ASK_AI_PROMPT } from "@/lib/site"
import { cn } from "@/lib/utils"

// Monochrome marks, 24×24, currentColor. Claude and OpenAI come from the hero's
// agent marks; Perplexity and Grok are added here (both from their brand kits).
const PATHS: Record<(typeof ASK_AI_ASSISTANTS)[number]["id"], string> = {
  chatgpt: AGENT_MARKS.find((m) => m.id === "openai")?.d ?? "",
  claude: AGENT_MARKS.find((m) => m.id === "claude")?.d ?? "",
  perplexity:
    "M22.3977 7.0896h-2.3106V.0676l-7.5094 6.3542V.1577h-1.1554v6.1966L4.4904 0v7.0896H1.6023v10.3976h2.8882V24l6.932-6.3591v6.2005h1.1554v-6.0469l6.9318 6.1807v-6.4879h2.8882V7.0896zm-3.4657-4.531v4.531h-5.355l5.355-4.531zm-13.2862.0676 4.8691 4.4634H5.6458V2.6262zM2.7576 16.332V8.245h7.8476l-6.1149 6.1147v1.9723H2.7576zm2.8882 5.0404v-3.8852h.0001v-2.6488l5.7763-5.7764v7.0111l-5.7764 5.2993zm7.0212-7.0212v-7.0111l5.7764 5.7764v2.6488h.0001v3.8852l-5.7765-5.2993zm8.9765 2.0444h-1.9727l-6.1149-6.1147h8.0876v6.1147z",
  grok: "M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815",
}

const PROMPT = encodeURIComponent(ASK_AI_PROMPT)

/**
 * "Ask about iii on" followed by one square button per chat assistant. Each
 * opens that assistant with the question prefilled, so the reader gets an
 * explanation of iii in the tool they already use. The label names iii once;
 * the marks are the buttons.
 */
export function AskAi({ className }: { className?: string }) {
  return (
    <div className={cn("flex items-center gap-3", className)}>
      <span className="text-[13px] text-gray-10">Ask about iii on</span>
      <ul className="flex items-center gap-1.5">
        {ASK_AI_ASSISTANTS.map((a) => (
          <li key={a.id}>
            <TrackedLink
              href={a.url(PROMPT)}
              target="_blank"
              rel="noopener noreferrer"
              aria-label={`Ask ${a.name} about iii (opens in a new tab)`}
              title={a.name}
              cta={{ cta_id: "ask_ai", cta_location: "footer", cta_label: a.id, cta_href: a.url("") }}
              className={cn(buttonVariants({ variant: "outline", size: "icon" }), "text-gray-11 hover:text-gray-12")}
            >
              <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" className="size-4">
                <path d={PATHS[a.id]} />
              </svg>
            </TrackedLink>
          </li>
        ))}
      </ul>
    </div>
  )
}
