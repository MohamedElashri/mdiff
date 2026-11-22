import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, renameSync } from "node:fs";
const root = new URL("../", import.meta.url);
const result = spawnSync(
  process.execPath,
  ["--import", "tsx", "scripts/extract-messages.ts"],
  {
    cwd: new URL("vendor/diffle/", root),
    encoding: "utf8",
    env: { ...process.env, TSX_TSCONFIG_PATH: "tsconfig.client.json" },
  },
);
if (result.status !== 0) {
  process.stderr.write(result.stderr);
  process.exit(result.status ?? 1);
}
const expected = JSON.parse(result.stdout);
const cataloguePath = new URL("vendor/diffle/src/client/i18n/en.json", root);
if (process.argv.includes("--write")) {
  const temporary = new URL("vendor/diffle/src/client/i18n/en.json.tmp", root);
  writeFileSync(temporary, JSON.stringify(expected, null, 2) + "\n");
  renameSync(temporary, cataloguePath);
  console.log(
    `English catalogue written: ${Object.keys(expected).length} messages.`,
  );
} else {
  const actual = JSON.parse(readFileSync(cataloguePath, "utf8"));
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    console.error("The English catalogue is stale. Run make messages.");
    process.exit(1);
  }
  console.log(
    `English catalogue verified: ${Object.keys(expected).length} messages.`,
  );
}
