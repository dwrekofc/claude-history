import { expect, test } from "bun:test";
import { planWorkspacePreview, type ManagerState, type PreviewRegistration } from "./manager";

const registrations: PreviewRegistration[] = [
  {
    name: "codex-live-preview-chat-a",
    workspaceId: "workspace-1",
    surfaceId: "chat-a",
    threadId: "thread-a",
    cwd: "/repo/a",
    url: "http://127.0.0.1:4777/",
  },
  {
    name: "codex-live-preview-chat-b",
    workspaceId: "workspace-1",
    surfaceId: "chat-b",
    threadId: "thread-b",
    cwd: "/repo/b",
    url: "http://127.0.0.1:4778/",
  },
];

test("active chat maps to the correct preview URL", () => {
  const state: ManagerState = { workspaceId: "workspace-1" };
  const plan = planWorkspacePreview(tree("chat-b", [chat("chat-a"), chat("chat-b")]), registrations, state);

  expect(plan.kind).toBe("open");
  expect(plan.url).toBe("http://127.0.0.1:4778/");
  expect(plan.sourceSurfaceId).toBe("chat-b");
  expect(plan.nextState.lastActiveChatSurfaceId).toBe("chat-b");
});

test("actual CMUX active field maps to the focused chat", () => {
  const state: ManagerState = { workspaceId: "workspace-1" };
  const plan = planWorkspacePreview(
    {
      active: { workspace_id: "workspace-1", surface_id: "chat-b" },
      windows: [
        {
          selected_workspace_id: "workspace-1",
          workspaces: [
            {
              id: "workspace-1",
              panes: [
                {
                  selected_surface_id: "chat-a",
                  surfaces: [
                    { ...chat("chat-a"), selected: true, focused: true },
                    chat("chat-b"),
                  ],
                },
              ],
            },
          ],
        },
      ],
    },
    registrations,
    state,
  );

  expect(plan.kind).toBe("open");
  expect(plan.url).toBe("http://127.0.0.1:4778/");
  expect(plan.nextState.lastActiveChatSurfaceId).toBe("chat-b");
});

test("clicking the browser preview keeps the last active chat", () => {
  const state: ManagerState = {
    workspaceId: "workspace-1",
    previewSurfaceId: "preview",
    lastActiveChatSurfaceId: "chat-a",
  };
  const plan = planWorkspacePreview(
    tree("preview", [chat("chat-a"), browser("preview", "http://127.0.0.1:4778/")]),
    registrations,
    state,
  );

  expect(plan.kind).toBe("navigate");
  expect(plan.url).toBe("http://127.0.0.1:4777/");
  expect(plan.previewSurfaceId).toBe("preview");
  expect(plan.nextState.lastActiveChatSurfaceId).toBe("chat-a");
});

test("closed preview pane is detected and reopened", () => {
  const state: ManagerState = {
    workspaceId: "workspace-1",
    previewSurfaceId: "closed-preview",
    lastActiveChatSurfaceId: "chat-a",
  };
  const plan = planWorkspacePreview(tree("chat-a", [chat("chat-a"), chat("chat-b")]), registrations, state);

  expect(plan.kind).toBe("open");
  expect(plan.url).toBe("http://127.0.0.1:4777/");
  expect(plan.sourceSurfaceId).toBe("chat-a");
  expect(plan.nextState.previewSurfaceId).toBeUndefined();
});

test("unregistered chats do not create duplicate tabs", () => {
  const state: ManagerState = {
    workspaceId: "workspace-1",
    previewSurfaceId: "preview",
    lastActiveChatSurfaceId: "chat-a",
  };
  const plan = planWorkspacePreview(
    tree("chat-c", [chat("chat-a"), chat("chat-c"), browser("preview", "http://127.0.0.1:4777/")]),
    registrations,
    state,
  );

  expect(plan.kind).toBe("idle");
  expect(plan.url).toBeUndefined();
});

function tree(focusedSurfaceId: string, surfaces: any[]) {
  return {
    focused: { surface_id: focusedSurfaceId },
    windows: [
      {
        id: "window-1",
        workspaces: [
          {
            id: "workspace-1",
            panes: [{ id: "pane-1", surfaces }],
          },
        ],
      },
    ],
  };
}

function chat(id: string) {
  return { id, type: "terminal", title: `Codex ${id}` };
}

function browser(id: string, url: string) {
  return { id, type: "browser", title: "Codex Live Markdown", url };
}
