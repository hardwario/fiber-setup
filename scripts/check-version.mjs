// Verify that the release tag and the versions in the repository agree: package.json, the
// workspace's Cargo.toml (the core, the CLI and the app take theirs from it, and so does Tauri)
// and the one the simulated FIBER shows (src/lib/api.ts).
// Usage: node scripts/check-version.mjs [v1.2.3]
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync("package.json", "utf8")).version;
const workspace = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync("Cargo.toml", "utf8"))?.[1];
// In GitHub Actions the ref is a tag only for releases; a branch name is no version.
const fromEnv = process.env.GITHUB_REF_TYPE === "tag" ? process.env.GITHUB_REF_NAME : undefined;
const tag = (process.argv[2] ?? fromEnv ?? "").replace(/^v/, "");

// The app's version there, not the image's ("2026-10-06, ...") a few lines above it.
const simulated = /^\s*version:\s*"(\d+\.\d+\.\d+[^"]*)"/m.exec(readFileSync("src/lib/api.ts", "utf8"))?.[1];

const problems = [];
if (pkg !== workspace) problems.push(`package.json ${pkg} != Cargo.toml (workspace) ${workspace}`);
if (pkg !== simulated) problems.push(`package.json ${pkg} != src/lib/api.ts (simulated) ${simulated}`);
if (tag && tag !== pkg) problems.push(`tag ${tag} != package.json ${pkg}`);
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`version ${pkg} OK`);
