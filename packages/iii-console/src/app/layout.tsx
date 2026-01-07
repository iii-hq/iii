import type { Metadata } from "next";
import { JetBrains_Mono } from "next/font/google";
import "./globals.css";
import { Sidebar } from "@/components/layout/Sidebar";

const jetbrainsMono = JetBrains_Mono({
  variable: "--font-mono",
  subsets: ["latin"],
  weight: ["400", "500", "600", "700"],
});

export const metadata: Metadata = {
  title: "iii Console",
  description: "Interoperable Invocation Interface Developer Console",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body
        className={`${jetbrainsMono.variable} font-mono antialiased flex h-screen overflow-hidden bg-background text-foreground`}
      >
        <Sidebar />
        <main className="flex-1 overflow-y-auto ml-56">
        {children}
        </main>
      </body>
    </html>
  );
}
