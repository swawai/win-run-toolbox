import { randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { copyFile, lstat, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import type { BuildArtifact } from "../../../build/launcher/_lib/artifact.ts";
import {
  ensureControlledDirectory,
  requireControlledDirectory,
} from "../../../build/_lib/release-set.ts";
import { moveFileReplace } from "../../../build/_lib/windows-filesystem.ts";

export async function publishLauncherTemplate(
  projHome: string,
  cacheDataRoot: string,
  artifact: BuildArtifact,
): Promise<{ changed: boolean; path: string }> {
  const favorites = await requireControlledDirectory(projHome, ["Favorites"], "Launcher templates");
  const target = join(favorites, "template.proj1.exe");
  await requireReplaceableTarget(target);
  if (await matches(target, artifact.length, artifact.sha256)) {
    return { changed: false, path: target };
  }
  const recovery = await ensureControlledDirectory(
    cacheDataRoot,
    ["retired", "launcher-template"],
    "Launcher template recovery",
  );
  const token = randomUUID().replaceAll("-", "");
  const stage = join(favorites, `.template.proj1.${token}.tmp`);
  const backup = join(recovery, `template.proj1.${token}.exe`);
  await copyFile(artifact.path, stage, constants.COPYFILE_EXCL);
  if (!await matches(stage, artifact.length, artifact.sha256)) {
    await rm(stage, { force: true });
    throw new Error("the staged Launcher template does not match its build manifest");
  }
  const targetExists = await exists(target);
  if (targetExists) {
    await copyFile(target, backup, constants.COPYFILE_EXCL);
    if (!await identical(target, backup)) {
      await rm(backup, { force: true });
      throw new Error(`the Launcher template recovery copy is corrupt: ${backup}`);
    }
  }
  try {
    moveFileReplace(stage, target);
  } catch (error) {
    throw new Error(
      `cannot atomically publish Launcher template; recovery files: '${stage}', '${backup}'; ${error}`,
    );
  }
  if (!await matches(target, artifact.length, artifact.sha256)) {
    if (targetExists) {
      moveFileReplace(backup, target);
    } else {
      await rm(target, { force: true });
    }
    throw new Error("the published Launcher template failed SHA-256 verification");
  }
  await rm(backup, { force: true });
  return { changed: true, path: target };
}

async function identical(left: string, right: string): Promise<boolean> {
  const [leftMetadata, rightMetadata, leftBytes, rightBytes] = await Promise.all([
    lstat(left),
    lstat(right),
    readFile(left),
    readFile(right),
  ]);
  return leftMetadata.isFile() && !leftMetadata.isSymbolicLink()
    && rightMetadata.isFile() && !rightMetadata.isSymbolicLink()
    && leftMetadata.size === rightMetadata.size
    && new Bun.CryptoHasher("sha256").update(leftBytes).digest("hex")
      === new Bun.CryptoHasher("sha256").update(rightBytes).digest("hex");
}

async function exists(path: string): Promise<boolean> {
  try {
    await lstat(path);
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
    throw error;
  }
}

async function matches(path: string, length: number, sha256: string): Promise<boolean> {
  try {
    const metadata = await lstat(path);
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size !== length) return false;
    return new Bun.CryptoHasher("sha256").update(await readFile(path)).digest("hex") === sha256;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
    throw error;
  }
}

async function requireReplaceableTarget(path: string): Promise<void> {
  try {
    const metadata = await lstat(path);
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      throw new Error(`Launcher template target is unsafe: ${path}`);
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
}
