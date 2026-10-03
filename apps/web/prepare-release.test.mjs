import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

test("release preparation builds the pinned export without touching the working tree", (t) => {
  const fixture = setup(t);
  const result = fixture.run();
  assert.equal(result.status, 0, result.stderr);
  const release = JSON.parse(result.stdout);
  assert.equal(release.source_commit, "pinned-website-commit");
  assert.equal(readFileSync(path.join(release.checkout_path, "dist/index.html"), "utf8"), "committed source");
  assert.equal(existsSync(path.join(release.checkout_path, "node_modules")), false);
  assert.equal(readFileSync(path.join(fixture.web, "dist/index.html"), "utf8"), "working tree build");
  assert.equal(readFileSync(path.join(fixture.web, "index.html"), "utf8"), "uncommitted source");
  assert.deepEqual(JSON.parse(readFileSync(path.join(release.checkout_path, ".openai/hosting.json"))), {
    static: { directory: "dist" }, project_id: "test-site",
  });
  assert.deepEqual(JSON.parse(readFileSync(path.join(path.dirname(release.checkout_path), "release.json"))), release);
  const calls = fixture.calls();
  assert.deepEqual(calls.filter((call) => call.command === "npm").map((call) => call.args), [
    ["ci", "--no-audit", "--no-fund"], ["run", "build"],
  ]);
  assert.ok(calls.filter((call) => call.command === "npm").every((call) => call.cwd === realpathSync(release.checkout_path)));
  assert.deepEqual(calls.filter((call) => call.command === "git").map((call) => call.args[0]), ["rev-parse", "archive"]);
  assert.ok(calls.find((call) => call.args[0] === "archive").args.includes(release.source_commit));
});

for (const failure of ["ci", "build"]) {
  test(`failed ${failure} does not return a release or continue preparation`, (t) => {
    const fixture = setup(t);
    const result = fixture.run(failure);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(`simulated ${failure} failure`));
    assert.equal(result.stdout, "");
    const npmCalls = fixture.calls().filter((call) => call.command === "npm");
    assert.equal(npmCalls.length, failure === "ci" ? 1 : 2);
    assert.equal(existsSync(path.join(path.dirname(npmCalls[0].cwd), "release.json")), false);
  });
}

function setup(t) {
  const root = mkdtempSync(path.join(tmpdir(), "savescummer-release-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const web = path.join(root, "repository/apps/web");
  const snapshot = path.join(root, "snapshot/apps/web");
  const bin = path.join(root, "bin");
  const temporary = path.join(root, "tmp");
  for (const directory of [web, snapshot, bin, temporary]) mkdirSync(directory, { recursive: true });
  for (const directory of [web, snapshot]) {
    mkdirSync(path.join(directory, ".openai"));
    writeFileSync(path.join(directory, ".openai/hosting.json"), JSON.stringify({
      static: { directory: "dist" }, ...(directory === web ? { project_id: "test-site" } : {}),
    }));
  }
  mkdirSync(path.join(web, "dist"));
  writeFileSync(path.join(web, "dist/index.html"), "working tree build");
  writeFileSync(path.join(web, "index.html"), "uncommitted source");
  writeFileSync(path.join(snapshot, "index.html"), "committed source");
  cpSync(new URL("./prepare-release.mjs", import.meta.url), path.join(web, "prepare-release.mjs"));
  const archive = path.join(root, "snapshot.tar");
  const tar = spawnSync("tar", ["-cf", archive, "-C", path.join(root, "snapshot"), "apps/web"], { encoding: "utf8" });
  assert.equal(tar.status, 0, tar.stderr);
  const log = path.join(root, "calls.jsonl");
  const mock = `#!${process.execPath}
const fs = require("node:fs");
const path = require("node:path");
const command = path.basename(process.argv[1]);
const args = process.argv.slice(2);
fs.appendFileSync(process.env.CALL_LOG, JSON.stringify({ command, args, cwd: process.cwd() }) + "\\n");
if (command === "git") {
  if (args[0] === "rev-parse") console.log("pinned-website-commit");
  else if (args[0] === "archive") fs.copyFileSync(process.env.SNAPSHOT_ARCHIVE, args.find(arg => arg.startsWith("--output=")).slice(9));
  else process.exit(1);
} else {
  const step = args[0] === "ci" ? "ci" : args[1];
  if (step === process.env.FAIL_STEP) { console.error("simulated " + step + " failure"); process.exit(1); }
  if (step === "ci") fs.mkdirSync("node_modules");
  if (step === "build") {
    if (!fs.existsSync("node_modules")) process.exit(1);
    fs.mkdirSync("dist");
    fs.copyFileSync("index.html", "dist/index.html");
  }
  console.log("npm " + step + " output");
}
`;
  for (const command of ["git", "npm"]) writeFileSync(path.join(bin, command), mock, { mode: 0o755 });
  return {
    web,
    calls: () => readFileSync(log, "utf8").trim().split("\n").map((line) => JSON.parse(line)),
    run: (failure = "") => spawnSync(process.execPath, [path.join(web, "prepare-release.mjs")], {
      encoding: "utf8",
      env: { ...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}`, TMPDIR: temporary,
        CALL_LOG: log, SNAPSHOT_ARCHIVE: archive, FAIL_STEP: failure },
    }),
  };
}
