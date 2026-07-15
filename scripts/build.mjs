import { copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const root = process.cwd();
const sourceDir = join(root, "apps", "desktop");
const distDir = join(sourceDir, "dist");

mkdirSync(distDir, { recursive: true });
for (const file of ["index.html", "styles.css", "app.js"]) {
  copyFileSync(join(sourceDir, file), join(distDir, file));
}

console.log("built Hot Trimmer focused desktop prototype");
