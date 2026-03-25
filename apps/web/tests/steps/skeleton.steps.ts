import { expect } from "@playwright/test";
import { Given, Then } from "../support/storybook.fixture";
import { storybookIframeUrl, toStoryId, extractOklchLightness } from "../support/storybook.helpers";

const SKELETON_SELECTOR = '[data-slot="skeleton"]';

Given(
  "Storybook is showing the Skeleton Default story in light mode",
  async ({ page, storybookUrl }) => {
    const storyId = toStoryId("skeleton", "default");
    const url = storybookIframeUrl(storybookUrl, { mode: "light" }, storyId);
    await page.goto(url);
    // Wait for skeleton to be attached (may be "hidden" due to animation/size)
    await page.locator(SKELETON_SELECTOR).first().waitFor({ state: "attached", timeout: 10000 });
  },
);

Given(
  "Storybook is showing the Skeleton Default story in dark mode",
  async ({ page, storybookUrl }) => {
    const storyId = toStoryId("skeleton", "default");
    const url = storybookIframeUrl(storybookUrl, { mode: "dark" }, storyId);
    await page.goto(url);
    await page.locator(SKELETON_SELECTOR).first().waitFor({ state: "attached", timeout: 10000 });
  },
);

Then("the skeleton element is visually distinguishable from the background", async ({ page }) => {
  const skeleton = page.locator(SKELETON_SELECTOR).first();
  // Element exists even if Playwright considers it "hidden" due to size/animation
  await expect(skeleton).toHaveCount(1);

  // Get computed background colors
  const skeletonBg = await skeleton.evaluate((el) => getComputedStyle(el).backgroundColor);

  // Get the bg-card wrapper's background
  const wrapperBg = await skeleton.evaluate((el) => {
    const wrapper = el.closest(".bg-card");
    if (wrapper) return getComputedStyle(wrapper).backgroundColor;
    return getComputedStyle(document.body).backgroundColor;
  });

  // Extract lightness values and verify contrast
  const skeletonLightness = extractOklchLightness(skeletonBg);
  const bgLightness = extractOklchLightness(wrapperBg);

  if (skeletonLightness !== null && bgLightness !== null) {
    const delta = Math.abs(skeletonLightness - bgLightness);
    expect(
      delta,
      `Skeleton lightness (${skeletonLightness}) too close to background (${bgLightness}), delta: ${delta}`,
    ).toBeGreaterThan(0.005);
  } else {
    // Fallback: verify the backgrounds differ as raw strings
    expect(skeletonBg, "Skeleton background should differ from page background").not.toBe(
      wrapperBg,
    );
  }
});
