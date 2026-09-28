import Link from "next/link"

/**
 * The export's 404.html. There is more than one root layout (the site, the
 * roadmap decks), so this page carries its own document. Kept dependency-free
 * and neutral: on iii.dev the CloudFront edge answers most unknown paths itself.
 */
export default function NotFound() {
  return (
    <html lang="en" className="dark">
      <body
        style={{
          margin: 0,
          background: "#0a0a0a",
          color: "#ededed",
          fontFamily: "ui-sans-serif, system-ui, sans-serif",
        }}
      >
        <main style={{ maxWidth: 560, margin: "20vh auto 0", padding: "0 20px" }}>
          <p style={{ fontSize: 13, color: "#8e8e8e", margin: 0 }}>404</p>
          <h1 style={{ fontSize: 28, fontWeight: 500, letterSpacing: "-0.02em", margin: "12px 0 8px" }}>
            Page not found
          </h1>
          <p style={{ fontSize: 15, lineHeight: 1.6, color: "#b5b5b5", margin: 0 }}>
            That URL does not exist on iii.dev.
          </p>
          <p style={{ marginTop: 24, fontSize: 15 }}>
            <Link href="/" style={{ color: "#ededed" }}>
              Home
            </Link>
            <span style={{ color: "#525252" }}> · </span>
            <a href="https://iii.dev/docs" style={{ color: "#ededed" }}>
              Docs
            </a>
            <span style={{ color: "#525252" }}> · </span>
            <Link href="/blog" style={{ color: "#ededed" }}>
              Blog
            </Link>
          </p>
        </main>
      </body>
    </html>
  )
}
