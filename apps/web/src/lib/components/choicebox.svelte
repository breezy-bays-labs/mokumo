<script lang="ts">
  import { cn } from "$lib/utils.js";

  interface Option {
    value: string;
    label: string;
    description?: string;
    disabled?: boolean;
  }

  interface Props {
    options: Option[];
    value?: string | string[];
    multiple?: boolean;
    class?: string;
    onchange?: (value: string | string[]) => void;
  }

  let {
    options,
    value = $bindable<string | string[] | undefined>(),
    multiple = false,
    class: className,
    onchange,
  }: Props = $props();

  function isSelected(optionValue: string): boolean {
    if (multiple && Array.isArray(value)) {
      return value.includes(optionValue);
    }
    return value === optionValue;
  }

  function select(optionValue: string) {
    if (multiple) {
      const current = Array.isArray(value) ? value : [];
      const next = current.includes(optionValue)
        ? current.filter((v) => v !== optionValue)
        : [...current, optionValue];
      value = next;
      onchange?.(next);
    } else {
      value = optionValue;
      onchange?.(optionValue);
    }
  }
</script>

<div
  class={cn("flex flex-col gap-2", className)}
  role={multiple ? "group" : "radiogroup"}
>
  {#each options as option (option.value)}
    <button
      type="button"
      role={multiple ? "checkbox" : "radio"}
      aria-checked={isSelected(option.value)}
      disabled={option.disabled}
      onclick={() => select(option.value)}
      class={cn(
        "flex w-full cursor-pointer flex-col gap-0.5 rounded-lg border px-4 py-3 text-left transition-colors",
        "hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
        isSelected(option.value)
          ? "border-primary bg-primary/5"
          : "border-border",
        option.disabled && "cursor-not-allowed opacity-50",
      )}
    >
      <span class="text-sm font-medium">{option.label}</span>
      {#if option.description}
        <span class="text-sm text-muted-foreground">{option.description}</span>
      {/if}
    </button>
  {/each}
</div>
