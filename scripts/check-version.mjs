// Verify that the release tag and the versions in the repository agree: package.json and the
// workspace's Cargo.toml (the core, the CLI and the app take theirs from it, and so does Tauri).
// Usage: node scripts/check-version.mjs [v1.2.3]
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync("package.json", "utf8")).version;
const workspace = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync("Cargo.toml", "utf8"))?.[1];
// In GitHub Actions the ref is a tag only for releases; a branch name is no version.
const fromEnv = process.env.GITHUB_REF_TYPE === "tag" ? process.env.GITHUB_REF_NAME : undefined;
const tag = (process.argv[2] ?? fromEnv ?? "").replace(/^v/, "");

const problems = [];
if (pkg !== workspace) problems.push(`package.json ${pkg} != Cargo.toml (workspace) ${workspace}`);
if (tag && tag !== pkg) problems.push(`tag ${tag} != package.json ${pkg}`);
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`version ${pkg} OK`);
