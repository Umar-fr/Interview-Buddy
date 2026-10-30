import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") {
  throw new Error("The Windows installer can only be prepared on Windows.");
}

const root = dirname(fileURLToPath(import.meta.url));
const target = join(root, "..", "src-tauri", "vulkan-runtime", "vulkan-1.dll");
const candidates = [
  process.env.VULKAN_SDK
    ? join(process.env.VULKAN_SDK, "Bin", "vulkan-1.dll")
    : null,
  join(process.env.SystemRoot ?? "C:\\Windows", "System32", "vulkan-1.dll"),
].filter(Boolean);
const source = candidates.find(existsSync);

if (!source) {
  throw new Error(
    "Vulkan loader not found. Install the Vulkan SDK before building the Windows installer.",
  );
}

mkdirSync(dirname(target), { recursive: true });
copyFileSync(source, target);
console.log(`Prepared Vulkan runtime from ${source}`);
