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
    ariaLabel?: string;
    ariaLabelledby?: string;
    class?: string;
    onchange?: (value: string | string[]) => void;
  }

  let {
    options,
    value = $bindable<string | string[] | undefined>(),
    multiple = false,
    ariaLabel,
    ariaLabelledby,
    class: className,
    onchange,
  }: Props = $props();

  let buttonRefs = $state<(HTMLButtonElement | null)[]>([]);

  function isSelected(optionValue: string): boolean {
    if (multiple && Array.isArray(value)) {
      return value.includes(optionValue);
    }
    return value === optionValue;
  }

  function getTabIndex(
    optionValue: string,
    index: number,
    disabled?: boolean,
  ): 0 | -1 {
    if (multiple) return 0;
    if (value === optionValue && !disabled) return 0;
    if (!value) {
      // focus first enabled option when nothing is selected
      const firstEnabled = options.findIndex((o) => !o.disabled);
      if (index === firstEnabled) return 0;
    }
    return -1;
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

  function handleKeyDown(e: KeyboardEvent, index: number) {
    if (multiple) return;
    const count = options.length;
    let next = index;
    if (e.key === "ArrowDown" || e.key === "ArrowRight") {
      e.preventDefault();
      next = (index + 1) % count;
    } else if (e.key === "ArrowUp" || e.key === "ArrowLeft") {
      e.preventDefault();
      next = (index - 1 + count) % count;
    } else {
      return;
    }
    // skip disabled options
    let attempts = 0;
    while (options[next]?.disabled && attempts < count) {
      next =
        e.key === "ArrowDown" || e.key === "ArrowRight"
          ? (next + 1) % count
          : (next - 1 + count) % count;
      attempts++;
    }
    if (!options[next]?.disabled) {
      buttonRefs[next]?.focus();
      select(options[next].value);
    }
  }
</script>

<div
  class={cn("flex flex-col gap-2", className)}
  role={multiple ? "group" : "radiogroup"}
  aria-label={ariaLabel}
  aria-labelledby={ariaLabelledby}
>
  {#each options as option, i (option.value)}
    <button
      bind:this={buttonRefs[i]}
      type="button"
      role={multiple ? "checkbox" : "radio"}
      aria-checked={isSelected(option.value)}
      tabindex={getTabIndex(option.value, i, option.disabled)}
      disabled={option.disabled}
      onclick={() => select(option.value)}
      onkeydown={(e) => handleKeyDown(e, i)}
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
