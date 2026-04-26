<script lang="ts">
  import {
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card/index.js";
  import {
    Alert,
    AlertDescription,
    AlertTitle,
  } from "$lib/components/ui/alert/index.js";
  import CheckCircle2 from "@lucide/svelte/icons/check-circle-2";
  import Circle from "@lucide/svelte/icons/circle";
  import { FALLBACK_BRANDING } from "$lib/branding";

  let { data } = $props();

  let branding = $derived(data.branding ?? FALLBACK_BRANDING);
  let overview = $derived(data.overview);
  let overviewLoaded = $derived(overview !== undefined);
  let isFreshInstall = $derived(overview?.fresh_install ?? false);
  // Missing system_health must NOT default to "ok" — that lies to the user
  // about platform health when the backend never reported it.
  let healthStatus = $derived<"ok" | "degraded" | "down" | "unknown">(
    overview?.system_health?.status ?? "unknown",
  );

  let bannerVisible = $state(false);

  // 3s threads the needle: long enough for `aria-live="polite"` to debounce
  // and announce, short enough to feel like a transient confirmation.
  const BANNER_DISPLAY_MS = 3000;

  function formatBackupAt(value: string | null | undefined): string {
    if (!value) return "—";
    const parsed = new Date(value);
    if (Number.isNaN(parsed.getTime())) return value;
    return parsed.toLocaleString();
  }

  $effect(() => {
    if (typeof document === "undefined") return;
    let timer: ReturnType<typeof setTimeout> | undefined;

    function showIfFlagged(): void {
      if (document.body.dataset.youreSetUp === "true") {
        bannerVisible = true;
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => {
          bannerVisible = false;
        }, BANNER_DISPLAY_MS);
      }
    }

    showIfFlagged();
    const obs = new MutationObserver(showIfFlagged);
    obs.observe(document.body, {
      attributes: true,
      attributeFilter: ["data-youre-set-up"],
    });

    return () => {
      obs.disconnect();
      if (timer) clearTimeout(timer);
    };
  });
</script>

<section
  data-testid="overview-body"
  class="space-y-6 p-8"
  style:--brand-bg={branding.tokens.bg}
  style:--brand-fg={branding.tokens.fg}
  style:--brand-primary={branding.tokens.primary}
  style:--brand-accent={branding.tokens.accent}
>
  <h1 class="text-2xl font-semibold">Overview</h1>

  {#if !overviewLoaded}
    <Card data-testid="overview-unavailable" class="max-w-2xl border-dashed">
      <CardHeader>
        <CardTitle>Dashboard data unavailable</CardTitle>
        <CardDescription>
          The {branding.appName} platform isn't reporting overview data yet. Check
          back once the shop server is online.
        </CardDescription>
      </CardHeader>
    </Card>
  {:else if isFreshInstall}
    <Card data-testid="get-started-panel" class="max-w-2xl">
      <CardHeader>
        <CardTitle>Get Started with {branding.appName}</CardTitle>
        <CardDescription>
          Three quick steps to set up your {branding.shopNounSingular}.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="space-y-3">
          {#each overview?.get_started_steps ?? [] as step (step.id)}
            <li
              data-checklist-step
              data-checklist-step-id={step.id}
              data-checklist-complete={step.complete}
              class="flex items-center gap-3"
            >
              {#if step.complete}
                <CheckCircle2 class="size-5 text-primary" aria-hidden="true" />
              {:else}
                <Circle
                  class="size-5 text-muted-foreground"
                  aria-hidden="true"
                />
              {/if}
              <span class="text-sm">{step.label}</span>
            </li>
          {/each}
        </ul>
      </CardContent>
    </Card>
  {:else}
    <div class="grid gap-6 md:grid-cols-2">
      <Card data-testid="overview-stat-strip">
        <CardHeader>
          <CardTitle>At a glance</CardTitle>
        </CardHeader>
        <CardContent>
          <dl class="grid grid-cols-2 gap-4">
            {#each overview?.stat_strip ?? [] as stat (stat.label)}
              <div>
                <dt
                  class="text-xs uppercase tracking-wide text-muted-foreground"
                >
                  {stat.label}
                </dt>
                <dd class="mt-1 text-2xl font-semibold">{stat.value}</dd>
              </div>
            {/each}
          </dl>
        </CardContent>
      </Card>

      <Card data-testid="overview-recent-activity">
        <CardHeader>
          <CardTitle>Recent activity</CardTitle>
        </CardHeader>
        <CardContent>
          <ul class="space-y-1">
            {#each overview?.recent_activity ?? [] as entry (entry.id)}
              <li>
                <a
                  data-activity-entry
                  data-activity-id={entry.id}
                  href={entry.href}
                  class="block rounded px-2 py-1.5 text-sm hover:bg-muted"
                >
                  {entry.label}
                </a>
              </li>
            {/each}
          </ul>
        </CardContent>
      </Card>

      <Card data-testid="overview-backups">
        <CardHeader>
          <CardTitle>Backups</CardTitle>
        </CardHeader>
        <CardContent>
          <dl class="space-y-2 text-sm">
            <div class="flex justify-between gap-3">
              <dt class="text-muted-foreground">Last</dt>
              <dd>{formatBackupAt(overview?.backups?.last_at)}</dd>
            </div>
            <div class="flex justify-between gap-3">
              <dt class="text-muted-foreground">Next</dt>
              <dd>{formatBackupAt(overview?.backups?.next_at)}</dd>
            </div>
          </dl>
        </CardContent>
      </Card>

      <Card data-testid="overview-system-health">
        <CardHeader>
          <CardTitle>System health</CardTitle>
        </CardHeader>
        <CardContent>
          {#if healthStatus === "ok"}
            <p class="flex items-center gap-2 text-sm">
              <span
                class="inline-block size-2 rounded-full bg-emerald-500"
                aria-hidden="true"
              ></span>
              All systems operational
            </p>
          {:else if healthStatus === "degraded"}
            <p class="flex items-center gap-2 text-sm text-amber-700">
              <span
                class="inline-block size-2 rounded-full bg-amber-500"
                aria-hidden="true"
              ></span>
              Degraded
            </p>
          {:else if healthStatus === "down"}
            <p class="flex items-center gap-2 text-sm text-destructive">
              <span
                class="inline-block size-2 rounded-full bg-destructive"
                aria-hidden="true"
              ></span>
              Down
            </p>
          {:else}
            <p class="flex items-center gap-2 text-sm text-muted-foreground">
              <span
                class="inline-block size-2 rounded-full bg-muted-foreground"
                aria-hidden="true"
              ></span>
              Status unknown
            </p>
          {/if}
        </CardContent>
      </Card>
    </div>
  {/if}

  {#if bannerVisible}
    <div
      data-testid="youre-set-up-banner"
      role="status"
      aria-live="polite"
      class="fixed right-6 bottom-6 z-50 max-w-sm"
    >
      <Alert class="border-emerald-500/50 bg-emerald-50 shadow-lg">
        <AlertTitle>You're set up!</AlertTitle>
        <AlertDescription>
          Your {branding.shopNounSingular} is ready to go.
        </AlertDescription>
      </Alert>
    </div>
  {/if}
</section>
