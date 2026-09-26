/**
 * Static export for Cloudflare Pages.
 *
 * Renders the moonshine route (a .crepus template parsed by the crepuscularity
 * WASM parser and rendered through @tschk/crepus-moonshine's React adapter)
 * to a single HTML document in dist/, next to the Tailwind-built stylesheet
 * and everything in public/.
 */
import { mkdirSync, cpSync, writeFileSync, readFileSync, renameSync } from "node:fs";
import { createHash } from "node:crypto";
import { renderToStaticMarkup } from "react-dom/server";
import Home from "../src/routes/index";

const body = renderToStaticMarkup(<Home />);

// Cache-bust: the Tailwind output gets a content hash in its filename, so a
// new deploy never pairs fresh HTML with a cached stylesheet.
const css = readFileSync("dist/styles.css");
const cssHash = createHash("sha256").update(css).digest("hex").slice(0, 8);
const cssName = `styles.${cssHash}.css`;
renameSync("dist/styles.css", `dist/${cssName}`);

const title = "Equilibrium — C FFI generation for C-compiling languages";
const description =
	"Load V, Zig, C, C++, C#, Rust, D, Nim, Odin, Hare, or TypeScript into Rust with one call. Equilibrium detects the language, compiles it to C, and binds it — plus consumer wrappers, rig, eqts, and eqswift.";
const canonical = "https://eq.tsc.hk/";

const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${title}</title>
<meta name="description" content="${description}" />
<link rel="canonical" href="${canonical}" />
<meta name="theme-color" content="#09090b" />
<meta name="generator" content="moonshine + crepuscularity" />
<link rel="icon" href="./favicon.svg" type="image/svg+xml" />
<link rel="stylesheet" href="./${cssName}" />
<meta property="og:type" content="website" />
<meta property="og:site_name" content="Equilibrium" />
<meta property="og:title" content="${title}" />
<meta property="og:description" content="${description}" />
<meta property="og:url" content="${canonical}" />
<meta name="twitter:card" content="summary" />
<meta name="twitter:title" content="${title}" />
<meta name="twitter:description" content="${description}" />
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "SoftwareSourceCode",
  "name": "Equilibrium",
  "description": ${JSON.stringify(description)},
  "codeRepository": "https://github.com/tschk/equilibrium",
  "programmingLanguage": "Rust",
  "license": "https://opensource.org/licenses/MIT",
  "url": "${canonical}"
}
</script>
</head>
<body>
${body}
</body>
</html>
`;

mkdirSync("dist", { recursive: true });
writeFileSync("dist/index.html", html);
cpSync("public", "dist", { recursive: true });
console.log(`dist/index.html (${(html.length / 1024).toFixed(1)} kB)`);
