import { expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  parseArgs,
  linkLocalMarkdownPaths,
  parseCodexSession,
  renderMarkdownDocumentHtml,
  renderSessionHtml,
  renderSessionMarkdown,
} from "./server";

test("defaults to automatic port selection", () => {
  const options = parseArgs([]);

  expect(options.port).toBeUndefined();
  expect(options.portStart).toBe(4777);
});

test("keeps explicit port overrides", () => {
  const options = parseArgs(["--port", "4999", "--port-start", "4888"]);

  expect(options.port).toBe(4999);
  expect(options.portStart).toBe(4888);
});

test("parses only chat messages from a Codex rollout", async () => {
  const dir = await mkdtemp(join(tmpdir(), "claude-history-live-"));
  const path = join(dir, "rollout-test.jsonl");
  await writeFile(
    path,
    [
      JSON.stringify({
        timestamp: "2026-05-01T16:00:00.000Z",
        type: "session_meta",
        payload: { id: "session-1", cwd: dir, cli_version: "0.128.0" },
      }),
      JSON.stringify({
        timestamp: "2026-05-01T16:00:01.000Z",
        type: "response_item",
        payload: {
          type: "message",
          role: "user",
          content: [{ type: "input_text", text: "<environment_context>ignore</environment_context>" }],
        },
      }),
      JSON.stringify({
        timestamp: "2026-05-01T16:00:02.000Z",
        type: "response_item",
        payload: {
          type: "message",
          role: "user",
          content: [{ type: "input_text", text: "Render docs/manifest.md:\n\n| A | B |\n| - | - |\n| 1 | 2 |" }],
        },
      }),
      JSON.stringify({
        timestamp: "2026-05-01T16:00:03.000Z",
        type: "response_item",
        payload: { type: "function_call_output", output: "tool output" },
      }),
      JSON.stringify({
        timestamp: "2026-05-01T16:00:04.000Z",
        type: "response_item",
        payload: {
          type: "message",
          role: "assistant",
          content: [{ type: "output_text", text: "Done with **markdown**.\n\n```sh\ncodex-live-preview start\n```" }],
        },
      }),
    ].join("\n"),
  );

  const session = await parseCodexSession(path);
  const markdown = renderSessionMarkdown(session);
  const html = renderSessionHtml(session, dir);

  expect(session.turns).toHaveLength(2);
  expect(markdown).toContain("## You");
  expect(markdown).toContain("## Codex");
  expect(markdown).not.toContain("environment_context");
  expect(markdown).not.toContain("tool output");
  expect(html).toContain("<table>");
  expect(html).toContain("<strong>markdown</strong>");
  expect(html).toContain("/file?path=");
  expect(html).toContain('class="copy-message"');
  expect(html).toContain('data-copy-index="0"');
  expect(html).toContain('class="copy-code"');
  expect(html).toContain("codex-live-preview start");

  await rm(dir, { recursive: true, force: true });
});

test("turns local markdown paths into preview links", () => {
  const linked = linkLocalMarkdownPaths(
    "Read README.md, customer-reference-db/docs/manifest.md, ./docs/local.md, docs/relative.md and /tmp/audit.md.",
    "/repo",
  );

  expect(linked).toContain("[README.md](/file?path=");
  expect(linked).toContain("[customer-reference-db/docs/manifest.md](/file?path=");
  expect(linked).toContain("[./docs/local.md](/file?path=");
  expect(linked).toContain("[docs/relative.md](/file?path=");
  expect(linked).toContain("[/tmp/audit.md](/file?path=");
});

test("rewrites markdown links that point to local markdown files", () => {
  const html = renderMarkdownDocumentHtml(
    "Open [the manifest](docs/manifest.md), [line](docs/manifest.md:111), and [absolute](/tmp/audit.md).",
    "/repo",
  );

  expect(html).toContain('href="/file?path=%2Frepo%2Fdocs%2Fmanifest.md&amp;href=docs%2Fmanifest.md"');
  expect(html).toContain('href="/file?path=%2Frepo%2Fdocs%2Fmanifest.md&amp;href=docs%2Fmanifest.md%3A111#L111"');
  expect(html).toContain('href="/file?path=%2Ftmp%2Faudit.md&amp;href=%2Ftmp%2Faudit.md"');
  expect(html).toContain('target="_blank"');
});

test("auto-links local markdown paths in normal markdown text", () => {
  const html = renderMarkdownDocumentHtml(
    "Read README.md, docs/manifest.md:111, ./docs/local.md, and /tmp/audit.md.",
    "/repo",
  );

  expect(html).toContain('href="/file?path=%2Frepo%2FREADME.md&amp;href=README.md"');
  expect(html).toContain('href="/file?path=%2Frepo%2Fdocs%2Fmanifest.md&amp;href=docs%2Fmanifest.md%3A111#L111"');
  expect(html).toContain('href="/file?path=%2Frepo%2Fdocs%2Flocal.md&amp;href=.%2Fdocs%2Flocal.md"');
  expect(html).toContain('href="/file?path=%2Ftmp%2Faudit.md&amp;href=%2Ftmp%2Faudit.md"');
});

test("does not auto-link markdown paths inside code", () => {
  const html = renderMarkdownDocumentHtml(
    "Inline `README.md` stays code.\n\n```sh\ncat docs/manifest.md\n```\n\nBut docs/linked.md links.",
    "/repo",
  );

  expect(html).toContain("<code>README.md</code>");
  expect(html).toContain("docs/manifest.md");
  expect(html).not.toContain('href="/file?path=%2Frepo%2FREADME.md');
  expect(html).not.toContain('href="/file?path=%2Frepo%2Fdocs%2Fmanifest.md');
  expect(html).toContain('href="/file?path=%2Frepo%2Fdocs%2Flinked.md&amp;href=docs%2Flinked.md"');
});
