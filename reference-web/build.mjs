import { cp, mkdir, readdir, realpath, stat, rm } from "node:fs/promises";
import { dirname, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { navigation } from './platform.js';

const root = dirname(fileURLToPath(import.meta.url));
const destination = resolve(root, "dist");
const appFiles = ["index.html", "not-found.html", "style.css", "app.js", "scene.js", "wheel.js", "platform.js"];
const extensions = new Set([".woff2", ".woff", ".ttf", ".png", ".jpg", ".jpeg", ".webp", ".avif", ".svg", ".ico"]);
await mkdir(destination, { recursive: true });
await rm(join(destination, "reference-data.json"), { force:true });
for (const file of appFiles) await cp(join(root, file), join(destination, file));
for (const path of ['site', ...navigation.filter(item=>item.id!=='home').map(item=>item.id)]) {
  await mkdir(join(destination,path),{recursive:true});
  await cp(join(root,'index.html'),join(destination,path,'index.html'));
}
let assets = 0;
async function copyPublic(directory, relative = "") {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.name.startsWith(".")) continue;
    if (entry.isSymbolicLink()) throw new Error("Public asset symlinks are not supported");
    const source = join(directory, entry.name);
    const target = join(destination, "public", relative, entry.name);
    if (entry.isDirectory()) await copyPublic(source, join(relative, entry.name));
    else if (entry.isFile() && (extensions.has(extname(entry.name)) || /^assets\/supa-(economy|social|trading|factory)-loop\.(webm|mp4)$/.test(join(relative, entry.name)) || join(relative, entry.name) === join("fonts", "OFL.txt"))) {
      const actual = await realpath(source);
      if (!actual.startsWith(`${resolve(root, "public")}${sep}`) || !(await stat(actual)).isFile()) throw new Error("Invalid public asset");
      await mkdir(dirname(target), { recursive: true });
      await cp(source, target);
      assets++;
    }
  }
}
try { await copyPublic(join(root, "public")); }
catch (error) { if (error.code !== "ENOENT") throw error; }
console.log(`Supa static assets built: ${appFiles.length} application files, ${assets} local assets`);
