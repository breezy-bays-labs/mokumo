import { createBdd } from "playwright-bdd";
import type { ServerInfoResponse } from "../../src/lib/types/ServerInfoResponse";
import { test as base } from "./storybook.fixture";
import { resolveWebRoot, startPreviewServer } from "./local-server";

type WorkerFixtures = {
  appUrl: string;
};

type TestFixtures = {
  lanTestState: {
    serverInfo: ServerInfoResponse | null;
  };
};

const webRoot = resolveWebRoot(import.meta.url);

export const test = base.extend<TestFixtures, WorkerFixtures>({
  appUrl: [
    async (_fixtures, use) => {
      const { server, url } = await startPreviewServer(webRoot);

      try {
        await use(url);
      } finally {
        server.kill("SIGTERM");
      }
    },
    { auto: true, scope: "worker" },
  ],

  lanTestState: async (_fixtures, use) => {
    await use({ serverInfo: null });
  },

  page: async ({ appUrl, page }, use) => {
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
      origin: appUrl,
    });

    await use(page);
  },
});

export const { Given, When, Then } = createBdd(test);
