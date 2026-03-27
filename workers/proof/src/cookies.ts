import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { BrowserSession } from "./types.js";

const execFileAsync = promisify(execFile);

type ExtractedCookie = {
  name: string;
  value: string;
  domain: string;
  path: string;
  expires?: number;
  secure: boolean;
  httpOnly: boolean;
  sameSite?: "Strict" | "Lax" | "None";
};

export async function extractAndInjectCookies(
  session: BrowserSession,
  targetUrl: string,
): Promise<number> {
  const hostname = new URL(targetUrl).hostname;
  const cookies = await extractCookiesForDomain(hostname);
  if (cookies.length === 0) return 0;

  const pwCookies = cookies.map((c) => ({
    name: c.name,
    value: c.value,
    domain: c.domain,
    path: c.path,
    expires: c.expires ?? -1,
    secure: c.secure,
    httpOnly: c.httpOnly,
    sameSite: (c.sameSite ?? "Lax") as "Strict" | "Lax" | "None",
  }));

  await session.context.addCookies(pwCookies);
  return pwCookies.length;
}

async function extractCookiesForDomain(domain: string): Promise<ExtractedCookie[]> {
  const cookies = await extractChromeCookies(domain);
  if (cookies.length > 0) return cookies;
  return extractFirefoxCookies(domain);
}

async function extractChromeCookies(domain: string): Promise<ExtractedCookie[]> {
  const platform = os.platform();
  let cookieDbPath: string;

  if (platform === "darwin") {
    cookieDbPath = path.join(os.homedir(), "Library/Application Support/Google/Chrome/Default/Cookies");
  } else if (platform === "linux") {
    cookieDbPath = path.join(os.homedir(), ".config/google-chrome/Default/Cookies");
  } else {
    return [];
  }

  if (!fs.existsSync(cookieDbPath)) return [];

  try {
    const { stdout } = await execFileAsync("sqlite3", [
      "-json",
      cookieDbPath,
      `SELECT name, value, host_key as domain, path, expires_utc, is_secure, is_httponly, samesite FROM cookies WHERE host_key LIKE '%${domain.replace(/'/g, "''")}'`,
    ]);

    if (!stdout.trim()) return [];

    const rows = JSON.parse(stdout) as Array<{
      name: string;
      value: string;
      domain: string;
      path: string;
      expires_utc: number;
      is_secure: number;
      is_httponly: number;
      samesite: number;
    }>;

    return rows
      .filter((r) => r.value)
      .map((r) => ({
        name: r.name,
        value: r.value,
        domain: r.domain,
        path: r.path,
        expires: r.expires_utc > 0 ? Math.floor((r.expires_utc / 1_000_000) - 11644473600) : undefined,
        secure: r.is_secure === 1,
        httpOnly: r.is_httponly === 1,
        sameSite: ([undefined, "Lax", "Strict", "None"] as const)[r.samesite] ?? undefined,
      }));
  } catch {
    return [];
  }
}

async function extractFirefoxCookies(domain: string): Promise<ExtractedCookie[]> {
  const platform = os.platform();
  let profilesDir: string;

  if (platform === "darwin") {
    profilesDir = path.join(os.homedir(), "Library/Application Support/Firefox/Profiles");
  } else if (platform === "linux") {
    profilesDir = path.join(os.homedir(), ".mozilla/firefox");
  } else {
    return [];
  }

  if (!fs.existsSync(profilesDir)) return [];

  let cookieDb: string | null = null;
  try {
    const profiles = fs.readdirSync(profilesDir);
    const defaultProfile = profiles.find((p) => p.endsWith(".default-release") || p.endsWith(".default"));
    if (defaultProfile) {
      const dbPath = path.join(profilesDir, defaultProfile, "cookies.sqlite");
      if (fs.existsSync(dbPath)) cookieDb = dbPath;
    }
  } catch {
    return [];
  }

  if (!cookieDb) return [];

  try {
    const { stdout } = await execFileAsync("sqlite3", [
      "-json",
      cookieDb,
      `SELECT name, value, host as domain, path, expiry, isSecure, isHttpOnly, sameSite FROM moz_cookies WHERE host LIKE '%${domain.replace(/'/g, "''")}'`,
    ]);

    if (!stdout.trim()) return [];

    const rows = JSON.parse(stdout) as Array<{
      name: string;
      value: string;
      domain: string;
      path: string;
      expiry: number;
      isSecure: number;
      isHttpOnly: number;
      sameSite: number;
    }>;

    return rows.filter((r) => r.value).map((r) => ({
      name: r.name,
      value: r.value,
      domain: r.domain,
      path: r.path,
      expires: r.expiry > 0 ? r.expiry : undefined,
      secure: r.isSecure === 1,
      httpOnly: r.isHttpOnly === 1,
      sameSite: (["None", "Lax", "Strict"] as const)[r.sameSite] ?? undefined,
    }));
  } catch {
    return [];
  }
}
