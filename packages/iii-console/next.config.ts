import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  /* config options here */
  reactStrictMode: true,
  // Suppress experimental warnings
  experimental: {
    // Use async page params handling
  },
};

export default nextConfig;
