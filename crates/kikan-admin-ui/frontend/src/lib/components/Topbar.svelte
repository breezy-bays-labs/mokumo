<script lang="ts">
  import { Tooltip } from "bits-ui";
  import HelpCircle from "@lucide/svelte/icons/help-circle";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import type { BrandingConfig } from "../branding";

  interface Props {
    branding: BrandingConfig;
    runningShops?: number;
  }

  let { branding, runningShops = 1 }: Props = $props();

  let canOpenShop = $derived(runningShops > 0);
  let tooltipMessage = $derived(`No ${branding.shopNounPlural} to open`);
</script>

<header
  data-testid="topbar"
  class="flex h-14 items-center justify-between gap-4 border-b border-border bg-background px-6"
  style:--brand-bg={branding.tokens.bg}
  style:--brand-fg={branding.tokens.fg}
  style:--brand-primary={branding.tokens.primary}
  style:--brand-accent={branding.tokens.accent}
>
  <div class="flex items-center gap-3">
    <span class="text-sm font-semibold text-foreground">Control Plane</span>
    <span
      data-testid="topbar-admin-badge"
      class="rounded bg-primary/10 px-2 py-0.5 text-xs font-semibold uppercase tracking-wide text-primary"
    >
      ADMIN
    </span>
    <span class="text-xs text-muted-foreground">
      {branding.appName} · {branding.shopNounPlural}
    </span>
  </div>

  <div class="flex items-center gap-2">
    <Tooltip.Provider delayDuration={0}>
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            {#if canOpenShop}
              <button
                type="button"
                data-testid="topbar-open-shop"
                {...props}
                class="flex items-center gap-1 rounded border border-border px-3 py-1.5 text-sm font-medium hover:bg-accent"
              >
                <ExternalLink class="size-4" />
                Open {branding.shopNounSingular}
              </button>
            {:else}
              <!-- Disabled <button> swallows pointer events, so the focusable
                   wrapper carries hover + keyboard focus to surface the tooltip. -->
              <span {...props} class="inline-block">
                <button
                  type="button"
                  data-testid="topbar-open-shop"
                  disabled
                  aria-disabled="true"
                  tabindex={-1}
                  class="flex items-center gap-1 rounded border border-border px-3 py-1.5 text-sm font-medium opacity-50 disabled:cursor-not-allowed"
                >
                  <ExternalLink class="size-4" />
                  Open {branding.shopNounSingular}
                </button>
              </span>
            {/if}
          {/snippet}
        </Tooltip.Trigger>
        {#if !canOpenShop}
          <Tooltip.Portal>
            <Tooltip.Content
              sideOffset={6}
              role="tooltip"
              aria-label={tooltipMessage}
              class="z-50 rounded bg-foreground px-3 py-1.5 text-xs text-background shadow"
            >
              {tooltipMessage}
            </Tooltip.Content>
          </Tooltip.Portal>
        {/if}
      </Tooltip.Root>
    </Tooltip.Provider>

    <button
      type="button"
      data-testid="topbar-help"
      class="flex items-center gap-1 rounded border border-border px-3 py-1.5 text-sm font-medium hover:bg-accent"
    >
      <HelpCircle class="size-4" />
      Help
    </button>
  </div>
</header>
