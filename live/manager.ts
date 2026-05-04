import { createConnection } from "node:net";
import { readdir, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export type PreviewRegistration = {
  name: string;
  workspaceId: string;
  surfaceId: string;
  threadId?: string;
  cwd: string;
  url: string;
};

export type ManagerState = {
  workspaceId: string;
  previewSurfaceId?: string;
  lastActiveChatSurfaceId?: string;
  lastUrl?: string;
};

export type PreviewPlan = {
  kind: "idle" | "open" | "navigate";
  url?: string;
  previewSurfaceId?: string;
  sourceSurfaceId?: string;
  nextState: ManagerState;
};

type ManagerOptions = {
  workspaceId: string;
  stateDir: string;
  pollMs: number;
};

type Surface = {
  id?: string;
  type?: string;
  title?: string;
  url?: string;
  active?: boolean;
  focused?: boolean;
  is_active?: boolean;
  isFocused?: boolean;
};

const PREVIEW_URL_PATTERN = /^http:\/\/127\.0\.0\.1:47\d+\//;

export function parseArgs(argv = process.argv.slice(2)): ManagerOptions {
  const options: ManagerOptions = {
    workspaceId: process.env.CMUX_WORKSPACE_ID || "",
    stateDir: defaultStateDir(),
    pollMs: 700,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];
    if (arg === "--workspace-id" && next) {
      options.workspaceId = next;
      index += 1;
    } else if (arg.startsWith("--workspace-id=")) {
      options.workspaceId = arg.slice("--workspace-id=".length);
    } else if (arg === "--state-dir" && next) {
      options.stateDir = next;
      index += 1;
    } else if (arg.startsWith("--state-dir=")) {
      options.stateDir = arg.slice("--state-dir=".length);
    } else if (arg === "--poll-ms" && next) {
      options.pollMs = Number(next);
      index += 1;
    } else if (arg.startsWith("--poll-ms=")) {
      options.pollMs = Number(arg.slice("--poll-ms=".length));
    }
  }

  if (!options.workspaceId) {
    throw new Error("CMUX_WORKSPACE_ID is required for the live preview workspace manager");
  }
  if (!Number.isFinite(options.pollMs) || options.pollMs < 250) {
    throw new Error(`Invalid --poll-ms value: ${options.pollMs}`);
  }

  return options;
}

export function planWorkspacePreview(
  tree: any,
  registrations: PreviewRegistration[],
  state: ManagerState,
): PreviewPlan {
  const workspace = findWorkspace(tree, state.workspaceId);
  const nextState: ManagerState = { ...state };
  if (!workspace) return { kind: "idle", nextState };

  const workspaceSurfaces = flattenSurfaces(workspace);
  const registered = registrations.filter((registration) => registration.workspaceId === state.workspaceId);
  const registeredBySurface = new Map(registered.map((registration) => [registration.surfaceId, registration]));
  const previewSurface = findPreviewSurface(workspaceSurfaces, registered, state.previewSurfaceId);
  nextState.previewSurfaceId = previewSurface?.id;

  const focusedSurfaceId = findFocusedSurfaceId(tree, workspace, workspaceSurfaces);
  const focusedRegistration = focusedSurfaceId ? registeredBySurface.get(focusedSurfaceId) : undefined;

  let targetRegistration = focusedRegistration;
  if (focusedRegistration) {
    nextState.lastActiveChatSurfaceId = focusedRegistration.surfaceId;
  } else if (focusedSurfaceId && isSameSurface(previewSurface, focusedSurfaceId)) {
    targetRegistration = nextState.lastActiveChatSurfaceId
      ? registeredBySurface.get(nextState.lastActiveChatSurfaceId)
      : undefined;
  } else {
    return { kind: "idle", nextState };
  }

  if (!targetRegistration) return { kind: "idle", nextState };

  nextState.lastUrl = targetRegistration.url;
  if (!previewSurface?.id) {
    return {
      kind: "open",
      url: targetRegistration.url,
      sourceSurfaceId: targetRegistration.surfaceId,
      nextState,
    };
  }

  if (previewSurface.url !== targetRegistration.url) {
    return {
      kind: "navigate",
      url: targetRegistration.url,
      previewSurfaceId: previewSurface.id,
      sourceSurfaceId: targetRegistration.surfaceId,
      nextState,
    };
  }

  return { kind: "idle", url: targetRegistration.url, previewSurfaceId: previewSurface.id, nextState };
}

async function runManager(options: ManagerOptions): Promise<void> {
  let state: ManagerState = { workspaceId: options.workspaceId };
  console.log(`Live preview workspace manager: ${options.workspaceId}`);

  for (;;) {
    try {
      const [tree, registrations] = await Promise.all([
        cmuxCall("system.tree"),
        readPreviewRegistrations(options.stateDir),
      ]);
      const plan = planWorkspacePreview(tree, registrations, state);
      state = plan.nextState;

      if (plan.kind === "open" && plan.url) {
        const result = await openPreviewSplit(plan.url, options.workspaceId, plan.sourceSurfaceId);
        state.previewSurfaceId = result?.surface_id || result?.surfaceId || state.previewSurfaceId;
        console.log(`Opened preview: ${plan.url}`);
      } else if (plan.kind === "navigate" && plan.url && plan.previewSurfaceId) {
        await cmuxCall("browser.navigate", { surface_id: plan.previewSurfaceId, url: plan.url });
        console.log(`Navigated preview: ${plan.url}`);
      }
    } catch (error) {
      console.error(error instanceof Error ? error.message : String(error));
    }

    await Bun.sleep(options.pollMs);
  }
}

async function readPreviewRegistrations(stateDir: string): Promise<PreviewRegistration[]> {
  if (!existsSync(stateDir)) return [];
  const entries = await readdir(stateDir, { withFileTypes: true });
  const metas = entries.filter((entry) => entry.isFile() && entry.name.startsWith("codex-live-preview-") && entry.name.endsWith(".meta"));
  const registrations = await Promise.all(
    metas.map(async (entry) => parsePreviewMeta(join(stateDir, entry.name)).catch(() => undefined)),
  );
  return registrations.filter((registration): registration is PreviewRegistration => Boolean(registration?.url));
}

async function parsePreviewMeta(path: string): Promise<PreviewRegistration | undefined> {
  const values = new Map<string, string>();
  const content = await readFile(path, "utf8");
  for (const line of content.split("\n")) {
    const equal = line.indexOf("=");
    if (equal === -1) continue;
    values.set(line.slice(0, equal), line.slice(equal + 1));
  }

  const name = values.get("name") || path.split("/").pop()?.replace(/\.meta$/, "") || "";
  const workspaceId = values.get("workspace_id") || values.get("cmux_workspace_id") || "";
  const surfaceId = values.get("surface_id") || values.get("cmux_surface_id") || "";
  const cwd = values.get("cwd") || "";
  const url = values.get("url") || "";
  if (!name || !workspaceId || !surfaceId || !cwd || !url) return undefined;

  return {
    name,
    workspaceId,
    surfaceId,
    threadId: values.get("thread_id") || values.get("codex_thread_id") || undefined,
    cwd,
    url,
  };
}

function findWorkspace(tree: any, workspaceId: string): any | undefined {
  const direct = tree?.workspace?.id === workspaceId ? tree.workspace : undefined;
  if (direct) return direct;
  return tree?.windows
    ?.flatMap((window: any) => window.workspaces || [])
    ?.find((workspace: any) => workspace.id === workspaceId);
}

function flattenSurfaces(container: any): Surface[] {
  const surfaces: Surface[] = [];
  const visit = (node: any) => {
    if (!node || typeof node !== "object") return;
    if (node.id && (node.type || node.url || node.title)) surfaces.push(node);
    for (const key of ["panes", "surfaces", "children", "tabs"]) {
      const children = node[key];
      if (Array.isArray(children)) children.forEach(visit);
    }
  };
  visit(container);
  return surfaces;
}

function findFocusedSurfaceId(tree: any, workspace: any, surfaces: Surface[]): string | undefined {
  const focused =
    (tree?.active?.workspace_id === workspace.id ? tree?.active?.surface_id : undefined) ||
    (tree?.active?.workspaceId === workspace.id ? tree?.active?.surfaceId : undefined) ||
    tree?.focused?.surface_id ||
    tree?.focused?.surfaceId ||
    tree?.focused_surface_id ||
    tree?.focusedSurfaceId ||
    workspace?.focused?.surface_id ||
    workspace?.focused?.surfaceId ||
    workspace?.focused_surface_id ||
    workspace?.focusedSurfaceId;
  if (focused && surfaces.some((surface) => surface.id === focused)) return focused;

  return surfaces.find((surface) => surface.active || surface.focused || surface.is_active || surface.isFocused)?.id;
}

function findPreviewSurface(
  surfaces: Surface[],
  registrations: PreviewRegistration[],
  preferredId?: string,
): Surface | undefined {
  const registeredUrls = new Set(registrations.map((registration) => registration.url));
  if (preferredId) {
    const preferred = surfaces.find((surface) => surface.id === preferredId);
    if (preferred) return preferred;
  }

  return surfaces.find((surface) => {
    if (surface.type !== "browser") return false;
    const title = String(surface.title || "");
    const url = String(surface.url || "");
    return title.includes("Codex Live Markdown") || registeredUrls.has(url) || PREVIEW_URL_PATTERN.test(url);
  });
}

function isSameSurface(surface: Surface | undefined, surfaceId: string): boolean {
  return Boolean(surface?.id && surface.id === surfaceId);
}

async function openPreviewSplit(url: string, workspaceId: string, sourceSurfaceId?: string): Promise<any> {
  return cmuxCall("browser.open_split", {
    url,
    direction: "right",
    workspace_id: workspaceId,
    ...(sourceSurfaceId ? { source_surface_id: sourceSurfaceId, surface_id: sourceSurfaceId } : {}),
  });
}

async function cmuxCall(method: string, params: Record<string, unknown> = {}): Promise<any> {
  const socketPath = process.env.CMUX_SOCKET_PATH || "/tmp/cmux.sock";
  const id = String(Date.now()) + Math.random().toString(16).slice(2);
  const request = JSON.stringify({ id, method, params }) + "\n";

  return new Promise((resolveCall, reject) => {
    const socket = createConnection(socketPath);
    let buffer = "";
    socket.setTimeout(5000);
    socket.on("connect", () => socket.write(request));
    socket.on("data", (chunk) => {
      buffer += chunk.toString();
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";
      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const response = JSON.parse(line);
          if (response.id !== id) continue;
          socket.destroy();
          if (response.error) {
            reject(new Error(JSON.stringify(response.error)));
          } else {
            resolveCall(response.result);
          }
        } catch {
          // Keep waiting for a complete JSON line.
        }
      }
    });
    socket.on("timeout", () => {
      socket.destroy();
      reject(new Error(`CMUX request timed out: ${method}`));
    });
    socket.on("error", reject);
  });
}

function defaultStateDir(): string {
  return join(process.env.XDG_STATE_HOME || join(homedir(), ".local/state"), "claude-history/live-preview");
}

if (import.meta.main) {
  runManager(parseArgs()).catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
