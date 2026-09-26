import { build } from "esbuild";
import { mkdir, copyFile, cp } from "node:fs/promises";
await mkdir("dist", { recursive: true });
await mkdir("dist/licenses", { recursive: true });
await Promise.all(["xterm", "addon-fit", "addon-unicode11"].map(name =>
  copyFile(`node_modules/@xterm/${name}/LICENSE`, `dist/licenses/${name}.txt`)));
await Promise.all([
  build({ entryPoints: ["runtime/index.ts"], outfile: "dist/index.js", bundle: true, format: "esm", target: "es2022" }),
  build({ entryPoints: ["runtime/worker.ts"], outfile: "dist/worker.js", bundle: true, format: "iife", target: "es2022" }),
  build({ entryPoints: ["runtime/frame.ts"], outfile: "dist/frame.js", bundle: true, format: "iife", target: "es2022",
    define: { "import.meta.url": '""' } }),
  copyFile("node_modules/@xterm/xterm/css/xterm.css", "dist/style.css"),
]);
await cp("dist", "web/runtime", { recursive: true });
await build({
  entryPoints: ["web/main.ts"], outfile: "web/main.js", bundle: true, format: "esm",
  plugins: [{ name: "public-runtime", setup(build) {
    build.onResolve({ filter: /^@tui2web\/runtime$/ }, () => ({ path: "./runtime/index.js", external: true }));
  } }], target: "es2022",
});
