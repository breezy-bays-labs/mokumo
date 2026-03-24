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
 * Navigate to a story iframe and wait for the default component to render.
 */
export async function gotoStory(
  page: Page,
  storybookUrl: string,
  globals?: Record<string, string>,
): Promise<void> {
  await page.goto(storybookIframeUrl(storybookUrl, globals));
  await page.locator(BUTTON_SELECTOR).first().waitFor();
}

/**
 * Convert a theme display name to a CSS class slug.
 */
export function toThemeSlug(theme: string): string {
  return theme.toLowerCase().replace(/\s+/g, "-");
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
 * Extract OKLCH lightness as a 0-1 value.
 * Chromium normalizes `oklch(0.195 ...)` to `oklch(19.5% ...)`.
 */
export function extractOklchLightness(value: string): number | null {
  const match = value.match(/oklch\(([\d.]+)(%?)/);
  if (!match) return null;
  const num = Number(match[1]);
  return match[2] === "%" ? num / 100 : num;
}
