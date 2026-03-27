import { test as base, createBdd } from "playwright-bdd";
import { resolve } from "node:path";
import { resolveWebRoot, startStaticServer } from "./local-server";

type WorkerFixtures = {
  storybookUrl: string;
};

const webRoot = resolveWebRoot(import.meta.url);

export const test = base.extend<object, WorkerFixtures>({
  storybookUrl: [
    // oxlint-disable-next-line eslint/no-empty-pattern -- playwright-bdd requires object destructuring
    async ({}, use) => {
      const { server, url } = await startStaticServer({
        outputDir: resolve(webRoot, "storybook-static"),
        outputName: "storybook-static",
        prepareCommand: "moon run web:build-storybook",
        webRoot,
      });

      try {
        await use(url);
      } finally {
        server.kill("SIGTERM");
      }
    },
    { auto: true, scope: "worker" },
  ],
});

export const { Given, When, Then } = createBdd(test);
