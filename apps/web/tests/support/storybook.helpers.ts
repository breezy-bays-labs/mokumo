import type { Page } from "@playwright/test";

export const DEFAULT_STORY_ID = "ui-button--default";
export const BUTTON_SELECTOR = '[data-slot="button"]';

/**
 * Build a Storybook iframe URL with optional globals.
 */
export function storybookIframeUrl(
  baseUrl: string,
  globals?: Record<string, string>,
  storyId = DEFAULT_STORY_ID,
): string {
  let url = `${baseUrl}/iframe.html?id=${storyId}&viewMode=story`;
  if (globals && Object.keys(globals).length > 0) {
    const globalsStr = Object.entries(globals)
      .map(([k, v]) => `${k}:${v}`)
      .join(";");
    url += `&globals=${globalsStr}`;
  }
  return url;
}

/**
 * Navigate to a story iframe and wait for a component to render.
 * Defaults to the Button story with its `data-slot="button"` selector.
 */
export async function gotoStory(
  page: Page,
  storybookUrl: string,
  globals?: Record<string, string>,
  options?: { storyId?: string; waitSelector?: string },
): Promise<void> {
  const storyId = options?.storyId ?? DEFAULT_STORY_ID;
  const waitSelector = options?.waitSelector ?? BUTTON_SELECTOR;
  await page.goto(storybookIframeUrl(storybookUrl, globals, storyId));
  await page.locator(waitSelector).first().waitFor();
}

/**
 * Convert a theme display name to a bare slug (e.g. "Midnight Bloom" → "midnight-bloom").
 * Does not include the `theme-` prefix — callers add it when needed.
 */
export function toThemeSlug(theme: string): string {
  return theme.toLowerCase().replace(/\s+/g, "-");
}

/**
 * Derive a Storybook story ID from a component display name.
 * Follows Storybook's title-to-ID convention for stories under "UI/".
 */
export function toStoryId(componentName: string, variant = "default"): string {
  return `ui-${toThemeSlug(componentName)}--${variant}`;
}

/**
 * Check whether the root element has a given CSS class.
 */
export async function rootHasClass(page: Page, className: string): Promise<boolean> {
  return page.evaluate((cls) => document.documentElement.classList.contains(cls), className);
}

/**
 * Get a CSS custom property value from the root element.
 */
export async function getCssVariableValue(page: Page, varName: string): Promise<string> {
  return page.evaluate(
    (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim(),
    varName,
  );
}

/**
 * Get the resolved font-family from the body element.
 */
export async function getBodyFontFamily(page: Page): Promise<string> {
  return page.evaluate(() => getComputedStyle(document.body).fontFamily);
}

/**
 * Extract OKLCH lightness as a 0-1 value.
 * Chromium normalizes `oklch(0.195 ...)` to `oklch(19.5% ...)`.
 */
export function extractOklchLightness(value: string): number | null {
  const match = value.match(/oklch\(([\d.]+)(%?)/);
  if (!match) return null;
  const num = Number(match[1]);
  return match[2] === "%" ? num / 100 : num;
}
