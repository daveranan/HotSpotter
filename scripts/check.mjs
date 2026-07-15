import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const required = [
  ".github/workflows/ci.yml",
  "Cargo.toml",
  "README.md",
  "docs/mvp-plan.md",
  "docs/phases.md",
  "docs/ux-workflow.md",
  "docs/adr/0001-native-shell-and-ownership.md",
  "docs/adr/0002-project-persistence.md",
  "docs/adr/0003-renderer-authority.md",
  "docs/adr/0004-color-and-channel-policy.md",
  "docs/adr/0005-source-file-ownership.md",
  "docs/phase-reports/phase-0.md",
  "docs/support/diagnostics.md",
  "docs/support/recovery.md",
  "apps/desktop/index.html",
  "apps/desktop/package.json",
  "apps/desktop/src/main.tsx",
  "apps/desktop/src-tauri/tauri.conf.json",
  "apps/desktop/src-tauri/src/lib.rs",
  "apps/desktop/styles.css",
  "crates/domain/src/lib.rs",
  "packages/ipc-contracts/src/index.ts",
  "fixtures/contracts/foundation-status.json",
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

const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
if (!Array.isArray(packageJson.workspaces) || !packageJson.workspaces.includes("apps/desktop")) {
  console.error("package.json must declare the native desktop workspace");
  process.exit(1);
}

const cargo = readFileSync(join(root, "Cargo.toml"), "utf8");
for (const member of ["crates/domain", "crates/project-store", "apps/desktop/src-tauri"]) {
  if (!cargo.includes(`\"${member}\"`)) {
    console.error(`Cargo.toml missing workspace member: ${member}`);
    process.exit(1);
  }
}

console.log(`checked ${required.length} Hot Trimmer Phase 0 foundation files`);
