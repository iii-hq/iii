import { cva } from "class-variance-authority"

// Redesign (v2) buttons. Press feedback is a 0.96 scale on :active (160ms
// ease-out); hover styles only apply on hover-capable pointers (Tailwind v4's
// `hover:` is gated by `@media (hover: hover)`). Focus rings stay neutral grey.
export const buttonVariants = cva(
  "group/button inline-flex shrink-0 cursor-pointer items-center justify-center gap-2 rounded-[10px] font-medium whitespace-nowrap outline-none select-none transition-[transform,background-color,color,box-shadow] duration-160 ease-out active:scale-[0.96] focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1 disabled:pointer-events-none disabled:opacity-50 motion-reduce:active:scale-100 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: "bg-gray-12 text-gray-1 hover:bg-gray-12/88",
        secondary: "bg-gray-3 text-gray-12 shadow-[inset_0_0_0_1px_var(--gray-6)] hover:bg-gray-4",
        outline: "bg-transparent text-gray-12 shadow-[inset_0_0_0_1px_var(--gray-6)] hover:bg-gray-3",
        ghost: "text-gray-11 hover:bg-gray-3 hover:text-gray-12",
        destructive: "bg-danger/10 text-danger hover:bg-danger/20",
        link: "text-gray-12 underline-offset-4 hover:underline active:scale-100",
      },
      size: {
        default: "h-9 px-3.5 text-sm",
        xs: "h-7 gap-1.5 rounded-[8px] px-2.5 text-xs",
        sm: "h-8 gap-1.5 rounded-[8px] px-3 text-[13px]",
        lg: "h-11 px-5 text-[15px]",
        icon: "size-9",
        "icon-xs": "size-7 rounded-[8px]",
        "icon-sm": "size-8 rounded-[8px]",
        "icon-lg": "size-11",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
)
