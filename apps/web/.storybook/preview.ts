// Polyfill matchMedia for Storybook test environment (fix #49)
// Must be at module top level — SidebarState creates IsMobile during init
if (typeof window !== "undefined" && !window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }) as MediaQueryList;
}

import "../src/app.css";
import type { Preview } from "@storybook/sveltekit";

const THEME_CLASSES = [
  "theme-tangerine",
  "theme-midnight-bloom",
  "theme-solar-dusk",
  "theme-soft-pop",
  "theme-sunset-horizon",
];

const MOKUMO_VIEWPORTS = {
  mobile: { name: "Mobile", styles: { width: "375px", height: "812px" } },
  sm: { name: "SM (640px)", styles: { width: "640px", height: "900px" } },
  md: { name: "MD (768px)", styles: { width: "768px", height: "900px" } },
  lg: { name: "LG (1024px)", styles: { width: "1024px", height: "900px" } },
  "2xl": { name: "2XL (1536px)", styles: { width: "1536px", height: "900px" } },
};

const preview: Preview = {
  parameters: {
    viewport: { viewports: MOKUMO_VIEWPORTS },
    a11y: { test: "warn" },
    chromatic: {
      modes: {
        light: { globals: { mode: "light" } },
        dark: { globals: { mode: "dark" } },
      },
    },
  },
  globalTypes: {
    mode: {
      name: "Mode",
      description: "Light or dark mode",
      toolbar: {
        icon: "moon",
        items: [
          { value: "light", title: "Light", icon: "sun" },
          { value: "dark", title: "Dark", icon: "moon" },
        ],
        dynamicTitle: true,
      },
    },
    theme: {
      name: "Theme",
      description: "Color theme",
      toolbar: {
        icon: "paintbrush",
        items: [
          { value: "niji", title: "Niji" },
          { value: "tangerine", title: "Tangerine" },
          { value: "midnight-bloom", title: "Midnight Bloom" },
          { value: "solar-dusk", title: "Solar Dusk" },
          { value: "soft-pop", title: "Soft Pop" },
          { value: "sunset-horizon", title: "Sunset Horizon" },
        ],
        dynamicTitle: true,
      },
    },
  },
  initialGlobals: {
    mode: "light",
    theme: "niji",
  },
  decorators: [
    (storyFn: () => unknown, context: { globals: Record<string, string> }) => {
      const mode = context.globals.mode || "light";
      const theme = context.globals.theme || "niji";
      const root = document.documentElement;

      root.classList.toggle("dark", mode === "dark");

      // Niji is the default — no theme class needed
      for (const cls of THEME_CLASSES) {
        root.classList.remove(cls);
      }
      if (theme !== "niji") {
        root.classList.add(`theme-${theme}`);
      }

      return storyFn();
    },
  ],
};

export default preview;
