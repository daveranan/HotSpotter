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
  "docs/phase-reports/phase-1.md",
  "docs/phase-reports/phase-2.md",
  "docs/support/diagnostics.md",
  "docs/support/recovery.md",
  "docs/support/security-review.md",
  "docs/technical-spec.md",
  "apps/desktop/index.html",
  "apps/desktop/package.json",
  "apps/desktop/src/main.tsx",
  "apps/desktop/src-tauri/tauri.conf.json",
  "apps/desktop/src-tauri/src/lib.rs",
  "apps/desktop/styles.css",
  "crates/domain/src/lib.rs",
  "packages/ipc-contracts/src/index.ts",
  "fixtures/contracts/foundation-status.json",
  "fixtures/contracts/phase-1-lifecycle.json",
  "fixtures/contracts/phase-2-patch-authoring.json",
  "fixtures/projects/schema-v1.sql",
  "fixtures/projects/migrate-v1-to-v2.sql",
  "fixtures/projects/schema-v2.sql",
  "fixtures/projects/migrate-v2-to-v3.sql",
  "fixtures/projects/schema-v3.sql",
  "fixtures/projects/migrate-v3-to-v4.sql",
  "fixtures/projects/schema-v4.sql",
  "fixtures/projects/migrate-v4-to-v5.sql",
  "fixtures/renders/phase-2-rectification-cases.json",
  "fixtures/projects/data-v1.sql",
];

const missing = required.filter((file) => !existsSync(join(root, file)));
if (missing.length > 0) {
  console.error(`missing files:\n${missing.join("\n")}`);
  process.exit(1);
}

const plan = readFileSync(join(root, "docs", "mvp-plan.md"), "utf8");
for (const term of ["Open Image", "Patches and Layout", "Create Trim Sheet", "ID Map", "Normal", "Roughness"]) {
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

const desktop = readFileSync(join(root, "apps", "desktop", "src", "main.tsx"), "utf8");
const styles = readFileSync(join(root, "apps", "desktop", "styles.css"), "utf8");
for (const marker of [
  'aria-label="Work modes"',
  'role="alertdialog"',
  'aria-modal="true"',
  'aria-live="polite"',
  'aria-label="Image import progress"',
]) {
  if (!desktop.includes(marker)) {
    console.error(`desktop shell missing accessibility contract: ${marker}`);
    process.exit(1);
  }
}
if (!styles.includes("prefers-reduced-motion: reduce")) {
  console.error("desktop shell must honor reduced motion");
  process.exit(1);
}

console.log(`checked ${required.length} Hot Trimmer foundation through Phase 2 gate files`);
