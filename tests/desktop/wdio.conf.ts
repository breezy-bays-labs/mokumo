import type { Options } from "@wdio/types";
import { spawn, type ChildProcess } from "node:child_process";
import { createConnection } from "node:net";
import { resolve } from "node:path";

// tauri-driver manages WebKitWebDriver (Linux) or EdgeDriver (Windows).
// It listens on port 4444 by default.
const TAURI_DRIVER_PORT = 4444;

let tauriDriver: ChildProcess;

function waitForPort(port: number, timeout: number): Promise<void> {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    function tryConnect() {
      const socket = createConnection({ port, host: "localhost" });
      socket.on("connect", () => {
        socket.destroy();
        resolve();
      });
      socket.on("error", () => {
        socket.destroy();
        if (Date.now() - start > timeout) {
          reject(new Error(`tauri-driver did not bind port ${port} within ${timeout}ms`));
        } else {
          setTimeout(tryConnect, 200);
        }
      });
    }
    tryConnect();
  });
}

export const config: Options.Testrunner = {
  runner: "local",

  specs: ["./specs/**/*.spec.ts"],
  maxInstances: 1, // Desktop app — one instance at a time

  capabilities: [
    {
      "browserName": "wry", // Tauri's webview engine
      "tauri:options": {
        application: resolve(
          import.meta.dirname,
          `../../target/debug/mokumo-desktop${process.platform === "win32" ? ".exe" : ""}`
        ),
      },
    } as WebdriverIO.Capabilities,
  ],

  hostname: "localhost",
  port: TAURI_DRIVER_PORT,

  framework: "mocha",
  mochaOpts: {
    ui: "bdd",
    timeout: 60_000, // Desktop startup can be slow in CI
  },

  reporters: ["spec"],

  // Start tauri-driver before tests, stop after
  onPrepare() {
    tauriDriver = spawn("tauri-driver", ["--port", String(TAURI_DRIVER_PORT)], {
      stdio: ["ignore", "pipe", "pipe"],
    });

    tauriDriver.on("error", (err) => {
      console.error(`[tauri-driver] Failed to spawn: ${err.message}`);
      console.error("Is tauri-driver installed? Run: cargo install tauri-driver");
      process.exit(1);
    });

    tauriDriver.stderr?.on("data", (data: Buffer) => {
      const msg = data.toString().trim();
      if (msg) console.error(`[tauri-driver] ${msg}`);
    });

    // Wait for tauri-driver to bind the port (up to 10s)
    return waitForPort(TAURI_DRIVER_PORT, 10_000);
  },

  onComplete() {
    tauriDriver?.kill();
  },
};
