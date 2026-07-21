import { existsSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";

const root = process.cwd();

function run(command, args, label, env = process.env) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit", shell: false, env });
  if (result.error) {
    console.error(`${label} could not start: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(`${label} failed with exit code ${result.status}`);
    process.exit(result.status ?? 1);
  }
}

function discoverBlender() {
  if (process.env.BLENDER_EXE) {
    const configured = path.resolve(process.env.BLENDER_EXE);
    if (!existsSync(configured)) {
      console.error(`BLENDER_EXE does not exist: ${configured}`);
      process.exit(1);
    }
    return configured;
  }
  const where = spawnSync("where.exe", ["blender.exe"], { encoding: "utf8", shell: false });
  if (where.status === 0) {
    const candidate = where.stdout.split(/\r?\n/).find(Boolean);
    if (candidate && existsSync(candidate)) return candidate;
  }
  const candidates = [];
  const foundation = "C:\\Program Files\\Blender Foundation";
  if (existsSync(foundation)) {
    for (const entry of readdirSync(foundation, { withFileTypes: true })) {
      if (entry.isDirectory()) candidates.push(path.join(foundation, entry.name, "blender.exe"));
    }
  }
  candidates.push(
    "C:\\Program Files\\Blender Foundation\\Blender\\blender.exe",
    "C:\\tmp\\hot-trimmer-blender-4.2\\blender-4.2.0-windows-x64\\blender.exe",
    path.join(process.env.LOCALAPPDATA ?? "", "Programs", "Blender Foundation", "Blender", "blender.exe"),
    path.join(process.env.USERPROFILE ?? "", "scoop", "apps", "blender", "current", "blender.exe"),
  );
  for (const drive of ["C", "D", "E", "F"]) {
    candidates.push(`${drive}:\\Program Files (x86)\\Steam\\steamapps\\common\\Blender\\blender.exe`);
    candidates.push(`${drive}:\\Program Files\\Steam\\steamapps\\common\\Blender\\blender.exe`);
    candidates.push(`${drive}:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe`);
  }
  return candidates.find((candidate) => candidate && existsSync(candidate));
}

function discoverPython() {
  if (process.env.PYTHON_EXE && existsSync(process.env.PYTHON_EXE)) return process.env.PYTHON_EXE;
  const command = process.platform === "win32" ? "where.exe" : "which";
  for (const name of process.platform === "win32" ? ["python.exe", "python3.exe"] : ["python3", "python"]) {
    const located = spawnSync(command, [name], { encoding: "utf8", shell: false });
    if (located.status === 0) {
      const candidate = located.stdout.split(/\r?\n/).find((entry) => entry && existsSync(entry));
      if (candidate) return candidate;
    }
  }
  const bundled = path.join(process.env.USERPROFILE ?? "", ".cache", "codex-runtimes", "codex-primary-runtime", "dependencies", "python", "python.exe");
  return existsSync(bundled) ? bundled : undefined;
}

const python = discoverPython();
if (!python) {
  console.error("Prompt 20B prerequisite missing: Python 3 was not found. Set PYTHON_EXE to a Python 3 executable.");
  process.exit(1);
}
run(
  python,
  ["-m", "unittest", "discover", "-s", "integrations/blender/hot_trimmer_companion/tests", "-p", "test_*.py"],
  "Prompt 20B pure-Python manifest/matching tests",
  { ...process.env, PYTHONPATH: path.join(root, "integrations", "blender") },
);

const blender = discoverBlender();
if (!blender) {
  console.error("Prompt 20B prerequisite missing: Blender was not found. Install Blender or set BLENDER_EXE to blender.exe; the real behavioral fixture is required and was not skipped.");
  process.exit(1);
}
run(
  blender,
  ["--background", "--factory-startup", "--python-exit-code", "1", "--python", "integrations/blender/hot_trimmer_companion/tests/blender_behavioral.py"],
  "Prompt 20B real headless-Blender behavioral fixture",
);
