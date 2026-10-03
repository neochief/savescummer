import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Export only committed website-branch files. Never alter the monorepo's Git state.
const webDirectory = path.dirname(fileURLToPath(import.meta.url));
const repository = path.resolve(webDirectory, "../..");
const manifest = JSON.parse(readFileSync(path.join(webDirectory, ".openai/hosting.json"), "utf8"));
if (!manifest.project_id) throw new Error("Register the Site before preparing a release.");

const commit = run("git", ["rev-parse", "--verify", "refs/heads/website^{commit}"]).trim();
const releaseDirectory = mkdtempSync(path.join(tmpdir(), "savescummer-website-"));
const checkout = path.join(releaseDirectory, "source");
const sourceArchive = path.join(releaseDirectory, "source.tar");
mkdirSync(checkout);
run("git", ["archive", "--format=tar", `--output=${sourceArchive}`, commit, "apps/web"]);
run("tar", ["-xf", sourceArchive, "-C", checkout, "--strip-components=2"]);

// Initial registration can precede the first release-branch commit of the Site ID.
const releaseManifestPath = path.join(checkout, ".openai/hosting.json");
const releaseManifest = JSON.parse(readFileSync(releaseManifestPath, "utf8"));
if (releaseManifest.project_id && releaseManifest.project_id !== manifest.project_id) {
  throw new Error("The website branch belongs to a different Site.");
}
releaseManifest.project_id = manifest.project_id;
writeFileSync(releaseManifestPath, JSON.stringify(releaseManifest, null, 2) + "\n");

// Build the pinned source in isolation; the working tree is never a release input.
process.stderr.write(run("npm", ["ci", "--no-audit", "--no-fund"], checkout));
process.stderr.write(run("npm", ["run", "build"], checkout));
rmSync(path.join(checkout, "node_modules"), { recursive: true, force: true });

const release = {
  source_branch: "website",
  source_commit: commit,
  project_id: manifest.project_id,
  checkout_path: checkout,
  archive_path: path.join(releaseDirectory, "deployment.tar.gz"),
};
writeFileSync(path.join(releaseDirectory, "release.json"), JSON.stringify(release, null, 2) + "\n");
console.log(JSON.stringify(release));

function run(command, args, cwd = repository) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim() || result.stdout.trim() || `${command} failed`);
  return result.stdout;
}
