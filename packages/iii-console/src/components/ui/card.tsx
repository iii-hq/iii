import * as React from "react"
import { clsx } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: (string | undefined | null | false)[]) {
  return twMerge(clsx(inputs))
}

export const Card = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    className={cn(
      "rounded border border-[#1D1D1D] bg-[#0A0A0A] text-[#F4F4F4]",
      className
    )}
    {...props}
  />
))
Card.displayName = "Card"

export const CardHeader = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    className={cn("flex flex-col space-y-1 p-4", className)}
    {...props}
  />
))
CardHeader.displayName = "CardHeader"

export const CardTitle = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLHeadingElement>
>(({ className, ...props }, ref) => (
  <h3
    ref={ref}
    className={cn(
      "text-xs font-medium tracking-wider uppercase text-[#5B5B5B]",
      className
    )}
    {...props}
  />
))
CardTitle.displayName = "CardTitle"

export const CardContent = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div ref={ref} className={cn("p-4 pt-0", className)} {...props} />
))
CardContent.displayName = "CardContent"

export function StatCard({ 
  title, 
  value, 
  subtitle,
  icon: Icon,
  trend,
  className 
}: { 
  title: string
  value: string | number
  subtitle?: string
  icon?: React.ComponentType<{ className?: string }>
  trend?: 'up' | 'down' | 'neutral'
  className?: string
}) {
  return (
    <Card className={cn("card-interactive", className)}>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle>{title}</CardTitle>
        {Icon && <Icon className="h-4 w-4 text-[#5B5B5B]" />}
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-semibold tracking-tight">{value}</div>
        {subtitle && (
          <p className="text-[10px] text-[#5B5B5B] uppercase tracking-wider mt-1">
            {subtitle}
          </p>
        )}
      </CardContent>
    </Card>
  )
}

export function Table({ children, className }: { children: React.ReactNode, className?: string }) {
  return (
    <div className={cn("overflow-x-auto", className)}>
      <table className="w-full text-xs">
        {children}
      </table>
    </div>
  )
}

export function TableHeader({ children }: { children: React.ReactNode }) {
  return (
    <thead className="border-b border-[#1D1D1D]">
      {children}
    </thead>
  )
}

export function TableBody({ children }: { children: React.ReactNode }) {
  return <tbody>{children}</tbody>
}

export function TableRow({ children, className }: { children: React.ReactNode, className?: string }) {
  return (
    <tr className={cn("border-b border-[#1D1D1D] transition-colors hover:bg-white/[0.02]", className)}>
      {children}
    </tr>
  )
}

export function TableHead({ children, className }: { children: React.ReactNode, className?: string }) {
  return (
    <th className={cn("text-left py-3 px-4 text-[10px] font-medium tracking-wider uppercase text-[#5B5B5B]", className)}>
      {children}
    </th>
  )
}

export function TableCell({ children, className }: { children: React.ReactNode, className?: string }) {
  return (
    <td className={cn("py-3 px-4 text-[#F4F4F4]", className)}>
      {children}
    </td>
  )
}

export function Badge({ 
  children, 
  variant = 'default',
  className 
}: { 
  children: React.ReactNode
  variant?: 'default' | 'success' | 'warning' | 'error' | 'outline' | 'accent'
  className?: string
}) {
  const variants = {
    default: 'bg-[#1D1D1D] text-[#F4F4F4]',
    success: 'bg-[#22C55E]/20 text-[#22C55E]',
    warning: 'bg-[#F3F724]/20 text-[#F3F724]',
    error: 'bg-[#EF4444]/20 text-[#EF4444]',
    outline: 'border border-[#1D1D1D] text-[#5B5B5B]',
    accent: 'bg-[#F3F724] text-black',
  }
  
  return (
    <span className={cn(
      "inline-flex items-center px-2 py-0.5 rounded text-[10px] font-medium tracking-wider uppercase",
      variants[variant],
      className
    )}>
      {children}
    </span>
  )
}

export function Button({ 
  children, 
  variant = 'primary',
  size = 'default',
  className,
  ...props 
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { 
  variant?: 'primary' | 'accent' | 'outline' | 'ghost'
  size?: 'sm' | 'default' | 'lg'
}) {
  const baseStyles = "inline-flex items-center justify-center gap-2 font-medium tracking-wider uppercase transition-all duration-150 whitespace-nowrap"
  
  const variants = {
    primary: 'bg-white text-black rounded-full hover:bg-[#F4F4F4]',
    accent: 'bg-[#F3F724] text-black rounded-full hover:brightness-90',
    outline: 'bg-transparent text-[#F4F4F4] border border-[#1D1D1D] rounded-full hover:bg-[#1D1D1D] hover:border-[#5B5B5B]',
    ghost: 'text-[#5B5B5B] hover:text-[#F4F4F4] hover:bg-[#1D1D1D] rounded',
  }
  
  const sizes = {
    sm: 'text-[10px] px-3 py-1.5',
    default: 'text-xs px-4 py-2',
    lg: 'text-sm px-6 py-3',
  }
  
  return (
    <button 
      className={cn(baseStyles, variants[variant], sizes[size], className)}
      {...props}
    >
      {children}
    </button>
  )
}

export function Input({ 
  className,
  ...props 
}: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        "w-full bg-[#1D1D1D] border border-[#1D1D1D] rounded px-3 py-2 text-xs text-[#F4F4F4] placeholder:text-[#5B5B5B] focus:outline-none focus:border-[#5B5B5B] transition-colors",
        className
      )}
      {...props}
    />
  )
}

export function Select({ 
  children,
  className,
  ...props 
}: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      className={cn(
        "bg-[#1D1D1D] border border-[#1D1D1D] rounded px-3 py-2 text-xs text-[#F4F4F4] focus:outline-none focus:border-[#5B5B5B] transition-colors appearance-none cursor-pointer",
        className
      )}
      {...props}
    >
      {children}
    </select>
  )
}

export function ActionBar({ children, className }: { children: React.ReactNode, className?: string }) {
  return (
    <div className={cn("flex items-center gap-2", className)}>
      {children}
    </div>
  )
}
