import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const required = [
  "README.md",
  "docs/mvp-plan.md",
  "docs/ux-workflow.md",
  "apps/desktop/index.html",
  "apps/desktop/styles.css",
  "apps/desktop/app.js",
];

const missing = required.filter((file) => !existsSync(join(root, file)));
if (missing.length > 0) {
  console.error(`missing files:\n${missing.join("\n")}`);
  process.exit(1);
}

const plan = readFileSync(join(root, "docs", "mvp-plan.md"), "utf8");
for (const term of ["Open image", "mark patches", "Create Trim Sheet", "ID Map", "Normal", "Roughness"]) {
  if (!plan.toLowerCase().includes(term.toLowerCase())) {
    console.error(`docs/mvp-plan.md missing required term: ${term}`);
    process.exit(1);
  }
}

console.log(`checked ${required.length} Hot Trimmer foundation files`);
