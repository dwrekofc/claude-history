import { createConnection } from "node:net";
import { readdir, readFile, realpath, stat } from "node:fs/promises";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { extname, isAbsolute, join, resolve } from "node:path";
import MarkdownIt from "markdown-it";
import hljs from "highlight.js";

type Role = "user" | "assistant";

type CodexTurn = {
  role: Role;
  label: string;
  text: string;
  timestamp?: string;
};

type CodexSession = {
  id?: string;
  path: string;
  cwd?: string;
  cliVersion?: string;
  timestamp?: string;
  modifiedMs: number;
  turns: CodexTurn[];
};

type CliOptions = {
  host: string;
  port: number;
  session?: string;
  projectDir: string;
  open: boolean;
  pollMs: number;
};

const md = new MarkdownIt({
  html: false,
  linkify: true,
  typographer: false,
  highlight(code, language) {
    const lang = language && hljs.getLanguage(language) ? language : "";
    if (lang) {
      return `<pre class="hljs"><code>${hljs.highlight(code, { language: lang }).value}</code></pre>`;
    }
    return `<pre class="hljs"><code>${md.utils.escapeHtml(code)}</code></pre>`;
  },
});

md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  const href = token.attrGet("href");
  if (href) {
    token.attrSet("href", normalizeLocalMarkdownHref(href, env?.baseDir || process.cwd()));
    if (token.attrGet("href")?.startsWith("/file?path=")) {
      token.attrSet("target", "_blank");
      token.attrSet("rel", "noopener noreferrer");
    }
  }
  return self.renderToken(tokens, idx, options);
};

export function parseArgs(argv = process.argv.slice(2)): CliOptions {
  const options: CliOptions = {
    host: "127.0.0.1",
    port: Number(process.env.LIVE_CHAT_PORT || 4777),
    projectDir: process.cwd(),
    open: false,
    pollMs: 1200,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];
    if (arg === "--open") {
      options.open = true;
    } else if (arg === "--no-open") {
      options.open = false;
    } else if (arg === "--port" && next) {
      options.port = Number(next);
      index += 1;
    } else if (arg.startsWith("--port=")) {
      options.port = Number(arg.slice("--port=".length));
    } else if (arg === "--host" && next) {
      options.host = next;
      index += 1;
    } else if (arg.startsWith("--host=")) {
      options.host = arg.slice("--host=".length);
    } else if (arg === "--session" && next) {
      options.session = next;
      index += 1;
    } else if (arg.startsWith("--session=")) {
      options.session = arg.slice("--session=".length);
    } else if (arg === "--project-dir" && next) {
      options.projectDir = next;
      index += 1;
    } else if (arg.startsWith("--project-dir=")) {
      options.projectDir = arg.slice("--project-dir=".length);
    } else if (arg === "--poll-ms" && next) {
      options.pollMs = Number(next);
      index += 1;
    } else if (arg.startsWith("--poll-ms=")) {
      options.pollMs = Number(arg.slice("--poll-ms=".length));
    }
  }

  if (!Number.isFinite(options.port) || options.port <= 0) {
    throw new Error(`Invalid --port value: ${options.port}`);
  }
  if (!Number.isFinite(options.pollMs) || options.pollMs < 250) {
    throw new Error(`Invalid --poll-ms value: ${options.pollMs}`);
  }

  options.projectDir = resolve(options.projectDir);
  return options;
}

export async function resolveSessionPath(options: CliOptions): Promise<string> {
  if (options.session && options.session !== "current" && options.session !== "latest") {
    const path = resolve(options.session);
    if (!existsSync(path)) {
      throw new Error(`Session file not found: ${path}`);
    }
    return path;
  }

  if (options.session !== "latest") {
    const threadId = process.env.CODEX_THREAD_ID;
    if (threadId) {
      const match = await findSessionById(threadId);
      if (match) return match;
    }
  }

  const projectMatch = await findLatestSessionForProject(options.projectDir);
  if (projectMatch) return projectMatch;

  const latest = await findLatestSession();
  if (latest) return latest;

  throw new Error(`No Codex rollout files found under ${codexSessionsDir()}`);
}

export async function parseCodexSession(path: string): Promise<CodexSession> {
  const [content, metadata] = await Promise.all([readFile(path, "utf8"), stat(path)]);
  const session: CodexSession = {
    path,
    modifiedMs: metadata.mtimeMs,
    turns: [],
  };

  for (const line of content.split("\n")) {
    if (!line.trim()) continue;
    let envelope: any;
    try {
      envelope = JSON.parse(line);
    } catch {
      continue;
    }

    const payload = envelope.payload;
    if (envelope.type === "session_meta" && payload) {
      session.id = payload.id ?? session.id;
      session.cwd = payload.cwd ?? session.cwd;
      session.cliVersion = payload.cli_version ?? session.cliVersion;
      session.timestamp = payload.timestamp ?? envelope.timestamp ?? session.timestamp;
      continue;
    }

    if (envelope.type !== "response_item" || payload?.type !== "message") {
      continue;
    }

    const role = payload.role === "user" || payload.role === "assistant" ? payload.role : undefined;
    if (!role) continue;

    const text = extractMessageText(payload.content).trim();
    if (!text || isAutomaticContextMessage(text)) continue;

    session.turns.push({
      role,
      label: role === "user" ? "You" : "Codex",
      text,
      timestamp: envelope.timestamp,
    });
  }

  return session;
}

export function renderSessionMarkdown(session: CodexSession): string {
  return session.turns.map((turn) => `## ${turn.label}\n\n${turn.text}`).join("\n\n");
}

export function renderMarkdownDocumentHtml(content: string, baseDir: string): string {
  return md.render(linkLocalMarkdownPaths(content, baseDir), { baseDir });
}

export function renderSessionHtml(session: CodexSession, projectDir = session.cwd || process.cwd()): string {
  return session.turns
    .map((turn, index) => {
      const rendered = md.render(linkLocalMarkdownPaths(turn.text, projectDir), { baseDir: projectDir });
      const timestamp = turn.timestamp ? escapeHtml(new Date(turn.timestamp).toLocaleTimeString()) : "";
      return `<article class="message ${turn.role}" data-index="${index}">
  <header><span>${escapeHtml(turn.label)}</span><time>${timestamp}</time></header>
  <div class="markdown-body">${rendered}</div>
</article>`;
    })
    .join("\n");
}

export function linkLocalMarkdownPaths(text: string, baseDir: string): string {
  const pathPattern = /(^|[\s(])((?:\/[^\s)<>]+|(?:\.{1,2}\/)?[A-Za-z0-9._@-][^\s)<>]*?)\.md)(?=[:.,;!?)]?(\s|$))/gm;
  return text.replace(pathPattern, (match, prefix: string, rawPath: string) => {
    if (match.includes("](") || rawPath.startsWith("http://") || rawPath.startsWith("https://")) {
      return match;
    }
    const cleanPath = rawPath.replace(/[.,;!?)]$/, "");
    const absolute = isAbsolute(cleanPath) ? cleanPath : resolve(baseDir, cleanPath);
    const href = `/file?path=${encodeURIComponent(absolute)}`;
    return `${prefix}[${cleanPath}](${href})`;
  });
}

function normalizeLocalMarkdownHref(href: string, baseDir: string): string {
  if (href.startsWith("/file?path=")) return href;
  if (href.startsWith("http://") || href.startsWith("https://") || href.startsWith("#")) return href;

  const [pathPart, hashPart] = href.split("#", 2);
  const decodedPath = decodeURIComponent(pathPart);
  const isMarkdown = [".md", ".markdown"].includes(extname(decodedPath).toLowerCase());
  if (!isMarkdown) return href;

  const absolute = isAbsolute(decodedPath) ? decodedPath : resolve(baseDir, decodedPath);
  const hash = hashPart ? `#${encodeURIComponent(hashPart)}` : "";
  return `/file?path=${encodeURIComponent(absolute)}${hash}`;
}

async function collectRolloutFiles(dir = codexSessionsDir()): Promise<string[]> {
  if (!existsSync(dir)) return [];
  const entries = await readdir(dir, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) return collectRolloutFiles(path);
      if (entry.isFile() && entry.name.startsWith("rollout-") && entry.name.endsWith(".jsonl")) {
        return [path];
      }
      return [];
    }),
  );
  return nested.flat();
}

async function findSessionById(id: string): Promise<string | undefined> {
  const files = await collectRolloutFiles();
  return files.find((file) => file.includes(id));
}

async function findLatestSessionForProject(projectDir: string): Promise<string | undefined> {
  const canonicalProject = await canonicalize(projectDir);
  const files = await collectRolloutFiles();
  const matches: { path: string; modifiedMs: number }[] = [];

  for (const path of files) {
    const session = await parseCodexSession(path);
    if (!session.cwd) continue;
    const cwd = await canonicalize(session.cwd);
    if (cwd === canonicalProject) {
      matches.push({ path, modifiedMs: session.modifiedMs });
    }
  }

  matches.sort((a, b) => b.modifiedMs - a.modifiedMs);
  return matches[0]?.path;
}

async function findLatestSession(): Promise<string | undefined> {
  const files = await collectRolloutFiles();
  const withStat = await Promise.all(
    files.map(async (path) => ({ path, modifiedMs: (await stat(path)).mtimeMs })),
  );
  withStat.sort((a, b) => b.modifiedMs - a.modifiedMs);
  return withStat[0]?.path;
}

async function canonicalize(path: string): Promise<string> {
  try {
    return await realpath(path);
  } catch {
    return resolve(path);
  }
}

function codexRoot(): string {
  return process.env.CODEX_HOME || join(homedir(), ".codex");
}

function codexSessionsDir(): string {
  return join(codexRoot(), "sessions");
}

function extractMessageText(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((block: any) => {
      if (typeof block?.text === "string") return block.text;
      if (typeof block?.content === "string") return block.content;
      if (block?.type === "input_image") return "[Image]";
      return "";
    })
    .filter(Boolean)
    .join("\n\n");
}

function isAutomaticContextMessage(text: string): boolean {
  const trimmed = text.trimStart();
  return (
    trimmed.startsWith("# AGENTS.md instructions for ") ||
    trimmed.startsWith("<environment_context>") ||
    trimmed.startsWith("<user_instructions>")
  );
}

function escapeHtml(input: string): string {
  return input
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
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
      for (const line of buffer.split("\n")) {
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

async function openInCmux(url: string): Promise<string> {
  if (!process.env.CMUX_SOCKET_PATH) {
    throw new Error("CMUX_SOCKET_PATH is not set");
  }

  try {
    await cmuxCall("browser.open_split", { url, direction: "right" });
    return "browser.open_split";
  } catch (openError) {
    const surfaces = await cmuxCall("surface.list");
    const browser = surfaces?.surfaces?.find((surface: any) => surface.type === "browser");
    if (!browser?.id) throw openError;
    await cmuxCall("browser.navigate", { surface_id: browser.id, url });
    return "browser.navigate";
  }
}

function chrome(title: string, body: string, meta = ""): string {
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${escapeHtml(title)}</title>
  <style>${css()}</style>
</head>
<body>
  <header class="topbar">
    <div class="title">
      <h1>${escapeHtml(title)}</h1>
      <div class="meta">${escapeHtml(meta)}</div>
    </div>
    <div class="toolbar" id="toolbar"></div>
  </header>
  <main>${body}</main>
</body>
</html>`;
}

function appHtml(pollMs: number): string {
  const body = `<div id="content"><div class="status">Waiting for transcript...</div></div>
  <script>
    const content = document.getElementById("content");
    const meta = document.querySelector(".meta");
    const toolbar = document.getElementById("toolbar");
    toolbar.innerHTML = '<button id="pause" type="button">Pause</button><button id="pin" class="active" type="button">Pin bottom</button><button id="raw" type="button">Raw</button>';
    const pause = document.getElementById("pause");
    const pin = document.getElementById("pin");
    const raw = document.getElementById("raw");
    let paused = false;
    let pinBottom = true;
    let rawMode = false;
    let lastSignature = "";

    pause.addEventListener("click", () => {
      paused = !paused;
      pause.classList.toggle("active", paused);
      pause.textContent = paused ? "Resume" : "Pause";
      if (!paused) refresh();
    });
    pin.addEventListener("click", () => {
      pinBottom = !pinBottom;
      pin.classList.toggle("active", pinBottom);
    });
    raw.addEventListener("click", () => {
      rawMode = !rawMode;
      raw.classList.toggle("active", rawMode);
      lastSignature = "";
      refresh();
    });

    async function refresh() {
      if (paused) return;
      const response = await fetch("/api/session", { cache: "no-store" });
      const data = await response.json();
      if (!response.ok) throw new Error(data.error || "Unable to load session");
      const signature = data.modifiedMs + ":" + data.turnCount + ":" + rawMode;
      if (signature === lastSignature) return;
      lastSignature = signature;
      meta.textContent = data.sessionPath + " | " + data.turnCount + " messages";
      content.innerHTML = rawMode
        ? "<article class='message'><div class='markdown-body'><pre><code></code></pre></div></article>"
        : data.html || "<div class='status'>No chat messages yet.</div>";
      if (rawMode) content.querySelector("code").textContent = data.markdown || "";
      if (pinBottom) window.scrollTo({ top: document.documentElement.scrollHeight });
    }

    refresh().catch((error) => {
      content.innerHTML = "<div class='status'>" + error.message + "</div>";
    });
    setInterval(() => refresh().catch(console.error), ${pollMs});
  </script>`;

  return chrome("Codex Live Markdown", body, "Connecting...");
}

function css(): string {
  return `
    :root {
      color-scheme: dark;
      --bg: #1c1c1e;
      --panel: #2c2c2e;
      --text: #f5f5f7;
      --muted: #98989d;
      --border: #3a3a3c;
      --accent: #0a84ff;
      --user: #0a84ff;
      --assistant: #3a3a3c;
      --assistant-text: #f5f5f7;
      --code: #111113;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      background: var(--bg);
      color: var(--text);
      font: 15px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    .topbar {
      position: sticky;
      top: 0;
      z-index: 2;
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 16px;
      align-items: center;
      min-height: 58px;
      padding: 10px 18px;
      background: color-mix(in srgb, #1c1c1e 92%, transparent);
      border-bottom: 1px solid var(--border);
      backdrop-filter: blur(10px);
    }
    .title { min-width: 0; }
    h1 {
      margin: 0;
      font-size: 15px;
      font-weight: 700;
      letter-spacing: 0;
    }
    .meta {
      overflow: hidden;
      color: var(--muted);
      font: 12px/1.4 ui-monospace, SFMono-Regular, Menlo, monospace;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .toolbar { display: flex; gap: 8px; align-items: center; }
    button {
      height: 32px;
      border: 1px solid var(--border);
      border-radius: 16px;
      background: var(--panel);
      color: var(--text);
      font: inherit;
      padding: 0 10px;
    }
    button.active {
      border-color: color-mix(in srgb, var(--accent) 60%, var(--border));
      color: var(--accent);
    }
    main {
      width: min(1040px, calc(100vw - 28px));
      margin: 0 auto;
      padding: 18px 0 60px;
    }
    .message {
      width: 100%;
      margin: 0 0 14px;
      border: 0;
      border-radius: 8px;
      background: var(--assistant);
      color: var(--assistant-text);
      overflow: hidden;
      box-shadow: 0 1px 1px rgb(0 0 0 / .18);
    }
    .message.user {
      margin-left: 0;
      margin-right: 0;
      background: var(--user);
      color: white;
    }
    .message > header {
      display: flex;
      justify-content: space-between;
      gap: 12px;
      padding: 8px 14px 0;
      border-bottom: 0;
      color: color-mix(in srgb, currentColor 70%, transparent);
      font-size: 12px;
      font-weight: 700;
      text-transform: uppercase;
    }
    .markdown-body { padding: 12px 14px 14px; overflow-wrap: anywhere; }
    .markdown-body > :first-child { margin-top: 0; }
    .markdown-body > :last-child { margin-bottom: 0; }
    .markdown-body h1, .markdown-body h2, .markdown-body h3 {
      margin: 1.1em 0 .45em;
      line-height: 1.25;
      letter-spacing: 0;
    }
    .markdown-body h1 { font-size: 1.55em; }
    .markdown-body h2 { font-size: 1.28em; }
    .markdown-body h3 { font-size: 1.08em; }
    .markdown-body table {
      display: block;
      width: 100%;
      overflow-x: auto;
      border-collapse: collapse;
      margin: 12px 0;
    }
    .markdown-body th, .markdown-body td {
      border: 1px solid color-mix(in srgb, currentColor 22%, transparent);
      padding: 6px 9px;
      vertical-align: top;
    }
    .markdown-body th {
      background: color-mix(in srgb, currentColor 10%, transparent);
      text-align: left;
    }
    .markdown-body pre {
      overflow-x: auto;
      margin: 12px 0;
      padding: 12px;
      border-radius: 6px;
      background: var(--code);
      color: #f8fafc;
    }
    .markdown-body code {
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      font-size: .92em;
    }
    .markdown-body :not(pre) > code {
      padding: 2px 4px;
      border-radius: 4px;
      background: color-mix(in srgb, currentColor 16%, transparent);
    }
    .markdown-body a { color: color-mix(in srgb, currentColor 78%, white); }
    .status { margin: 32px auto; color: var(--muted); text-align: center; }
    @media (max-width: 720px) {
      .topbar { grid-template-columns: 1fr; }
      .toolbar { justify-content: flex-start; flex-wrap: wrap; }
      main { width: calc(100vw - 18px); }
    }`;
}

async function main() {
  const options = parseArgs();
  const sessionPath = await resolveSessionPath(options);
  const url = `http://${options.host}:${options.port}/`;

  const server = Bun.serve({
    hostname: options.host,
    port: options.port,
    async fetch(request) {
      const requestUrl = new URL(request.url);
      if (requestUrl.pathname === "/api/session") {
        try {
          const session = await parseCodexSession(sessionPath);
          return jsonResponse({
            id: session.id,
            cwd: session.cwd,
            sessionPath: session.path,
            modifiedMs: session.modifiedMs,
            turnCount: session.turns.length,
            markdown: renderSessionMarkdown(session),
            html: renderSessionHtml(session, session.cwd || options.projectDir),
          });
        } catch (error) {
          return jsonResponse({ error: error instanceof Error ? error.message : String(error) }, 500);
        }
      }
      if (requestUrl.pathname === "/file") {
        const path = requestUrl.searchParams.get("path");
        if (!path) return new Response("Missing path", { status: 400 });
        try {
          const absolute = resolve(path);
          const content = await readFile(absolute, "utf8");
          const fileExt = extname(absolute).toLowerCase();
          const rendered =
            fileExt === ".md" || fileExt === ".markdown"
              ? renderMarkdownDocumentHtml(content, resolve(absolute, ".."))
              : `<pre><code>${escapeHtml(content)}</code></pre>`;
          return new Response(chrome(absolute.split("/").pop() || "File", `<article class="message"><div class="markdown-body">${rendered}</div></article>`, absolute), {
            headers: { "content-type": "text/html; charset=utf-8" },
          });
        } catch (error) {
          return new Response(chrome("Unable to Open File", `<div class="status">${escapeHtml(error instanceof Error ? error.message : String(error))}</div>`, path), {
            status: 404,
            headers: { "content-type": "text/html; charset=utf-8" },
          });
        }
      }
      if (requestUrl.pathname === "/" || requestUrl.pathname === "/index.html") {
        return new Response(appHtml(options.pollMs), {
          headers: { "content-type": "text/html; charset=utf-8" },
        });
      }
      return new Response("Not found", { status: 404 });
    },
  });

  console.log(`Live Codex Markdown: ${url}`);
  console.log(`Session: ${sessionPath}`);

  if (options.open) {
    try {
      const method = await openInCmux(url);
      console.log(`Opened in CMUX via ${method}`);
    } catch (error) {
      console.error(`Could not open CMUX browser: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  process.on("SIGINT", () => {
    server.stop();
    process.exit(0);
  });
}

if (import.meta.main) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
