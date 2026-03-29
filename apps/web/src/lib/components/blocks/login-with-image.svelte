<script lang="ts">
  import { cn } from "$lib/utils.js";
  import Button from "$lib/components/ui/button/button.svelte";
  import Input from "$lib/components/ui/input/input.svelte";
  import Label from "$lib/components/ui/label/label.svelte";
  import type { Snippet } from "svelte";

  interface Props {
    title?: string;
    subtitle?: string;
    email?: string;
    password?: string;
    imageSrc?: string;
    imageAlt?: string;
    onsubmit?: (e: SubmitEvent) => void;
    footer?: Snippet;
    class?: string;
  }

  const uid = crypto.randomUUID().slice(0, 8);

  let {
    title = "Sign in",
    subtitle = "Enter your credentials to continue.",
    email = $bindable(""),
    password = $bindable(""),
    imageSrc,
    imageAlt = "Brand image",
    onsubmit,
    footer,
    class: className,
  }: Props = $props();
</script>

<div class={cn("grid min-h-[600px] lg:grid-cols-2", className)}>
  <div class="flex items-center justify-center p-6 lg:p-10">
    <div class="w-full max-w-sm space-y-6">
      <div class="space-y-2 text-center">
        <h1 class="text-2xl font-bold tracking-tight">{title}</h1>
        <p class="text-sm text-muted-foreground">{subtitle}</p>
      </div>

      <form class="space-y-4" {onsubmit}>
        <div class="space-y-2">
          <Label for="login-email-{uid}">Email</Label>
          <Input
            id="login-email-{uid}"
            name="email"
            type="email"
            placeholder="you@example.com"
            autocomplete="email"
            required
            bind:value={email}
          />
        </div>
        <div class="space-y-2">
          <div class="flex items-center justify-between">
            <Label for="login-password-{uid}">Password</Label>
            <a
              href="/forgot-password"
              class="text-xs text-muted-foreground hover:text-foreground underline-offset-4 hover:underline"
            >
              Forgot password?
            </a>
          </div>
          <Input
            id="login-password-{uid}"
            name="password"
            type="password"
            placeholder="Password"
            autocomplete="current-password"
            required
            bind:value={password}
          />
        </div>
        <Button type="submit" class="w-full">Sign in</Button>
      </form>

      {#if footer}
        <div class="text-center text-sm text-muted-foreground">
          {@render footer()}
        </div>
      {/if}
    </div>
  </div>

  <div class="relative hidden bg-muted lg:block">
    {#if imageSrc}
      <img
        src={imageSrc}
        alt={imageAlt}
        class="absolute inset-0 h-full w-full object-cover"
      />
    {:else}
      <div
        class="flex h-full items-center justify-center bg-gradient-to-br from-primary/20 to-primary/5"
      >
        <div class="space-y-2 text-center">
          <div class="text-4xl font-bold text-primary/60">Mokumo</div>
          <div class="text-sm text-muted-foreground">Production Management</div>
        </div>
      </div>
    {/if}
  </div>
</div>
