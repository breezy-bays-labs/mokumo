import type { Options } from "@wdio/types";
import { spawn, type ChildProcess } from "node:child_process";
import { createConnection } from "node:net";
import { homedir } from "node:os";
import { resolve } from "node:path";

// tauri-driver manages WebKitWebDriver (Linux) or EdgeDriver (Windows).
// It listens on port 4444 by default.
const TAURI_DRIVER_PORT = 4444;

let tauriDriver: ChildProcess;
let shouldExit = false;

function waitForPort(port: number, timeout: number): Promise<void> {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    function tryConnect() {
      const socket = createConnection({ port, host: "127.0.0.1" });
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
  maxInstances: 1,

  // Official Tauri v2 WebDriverIO config: no browserName, only tauri:options.
  // tauri-driver proxies to the platform's native WebDriver (WebKitWebDriver
  // on Linux, EdgeDriver on Windows) and handles the "wry" mapping internally.
  capabilities: [
    {
      maxInstances: 1,
      "tauri:options": {
        application: resolve(
          import.meta.dirname,
          `../../target/debug/mokumo-desktop${process.platform === "win32" ? ".exe" : ""}`
        ),
      },
    } as WebdriverIO.Capabilities,
  ],

  hostname: "127.0.0.1",
  port: TAURI_DRIVER_PORT,

  framework: "mocha",
  mochaOpts: {
    ui: "bdd",
    timeout: 60_000,
  },

  reporters: ["spec"],

  // Spawn tauri-driver before each session (per official Tauri docs).
  // Using beforeSession instead of onPrepare ensures a fresh driver per session.
  beforeSession() {
    const driverPath = resolve(homedir(), ".cargo", "bin", "tauri-driver");
    tauriDriver = spawn(driverPath, ["--port", String(TAURI_DRIVER_PORT)], {
      stdio: ["ignore", process.stdout, process.stderr],
    });

    tauriDriver.on("error", (err) => {
      console.error(`[tauri-driver] Failed to spawn: ${err.message}`);
      console.error("Is tauri-driver installed? Run: cargo install tauri-driver");
      process.exit(1);
    });

    tauriDriver.on("exit", (code) => {
      if (!shouldExit) {
        console.error(`[tauri-driver] exited unexpectedly with code: ${code}`);
        process.exit(1);
      }
    });

    return waitForPort(TAURI_DRIVER_PORT, 10_000);
  },

  afterSession() {
    shouldExit = true;
    tauriDriver?.kill();
  },
};
