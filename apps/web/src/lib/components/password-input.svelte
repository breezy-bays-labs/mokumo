<script lang="ts">
  import { Input } from "$lib/components/ui/input";
  import { Button } from "$lib/components/ui/button";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";

  interface Props {
    value: string;
    placeholder?: string;
    showStrength?: boolean;
    id?: string;
  }

  let {
    value = $bindable(""),
    placeholder = "Password",
    showStrength = false,
    id,
  }: Props = $props();

  let visible = $state(false);

  let strength = $derived.by(() => {
    if (!showStrength || !value)
      return { score: 0, rules: [] as { label: string; met: boolean }[] };
    const rules = [
      { label: "8+ characters", met: value.length >= 8 },
      { label: "Uppercase", met: /[A-Z]/.test(value) },
      { label: "Lowercase", met: /[a-z]/.test(value) },
      { label: "Number", met: /[0-9]/.test(value) },
      { label: "Special char", met: /[^A-Za-z0-9]/.test(value) },
    ];
    const score = rules.filter((r) => r.met).length;
    return { score, rules };
  });

  let strengthColor = $derived(
    strength.score <= 1
      ? "bg-destructive"
      : strength.score <= 3
        ? "bg-yellow-500"
        : "bg-green-500",
  );
</script>

<div class="space-y-2">
  <div class="relative">
    <Input type={visible ? "text" : "password"} bind:value {placeholder} {id} />
    <Button
      type="button"
      variant="ghost"
      size="icon"
      class="absolute right-0 top-0 h-8 w-8 hover:bg-transparent"
      onclick={() => (visible = !visible)}
    >
      {#if visible}
        <EyeOff class="h-4 w-4 text-muted-foreground" />
      {:else}
        <Eye class="h-4 w-4 text-muted-foreground" />
      {/if}
      <span class="sr-only">{visible ? "Hide" : "Show"} password</span>
    </Button>
  </div>
  {#if showStrength && value}
    <div class="space-y-1.5">
      <div class="flex gap-1">
        {#each Array(5) as _, i}
          <div
            class="h-1 flex-1 rounded-full transition-colors {i < strength.score
              ? strengthColor
              : 'bg-muted'}"
          ></div>
        {/each}
      </div>
      <div
        class="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-muted-foreground"
      >
        {#each strength.rules as rule}
          <span class={rule.met ? "text-green-600 dark:text-green-400" : ""}>
            {rule.met ? "\u2713" : "\u2717"}
            {rule.label}
          </span>
        {/each}
      </div>
    </div>
  {/if}
</div>
