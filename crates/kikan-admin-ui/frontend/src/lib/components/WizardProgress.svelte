<script lang="ts">
  export interface WizardStep {
    id: string;
    label: string;
  }

  interface Props {
    steps: WizardStep[];
    currentId: string;
    testId: string;
    /** data-testid prefix for clickable step buttons (e.g. "wizard-step"). */
    stepTestidPrefix: string;
    onSelect?: (id: string) => void;
  }

  let { steps, currentId, testId, stepTestidPrefix, onSelect }: Props = $props();
</script>

<ol data-testid={testId} class="flex items-center gap-2">
  {#each steps as step, i (step.id)}
    {@const active = step.id === currentId}
    <li data-step data-step-id={step.id} class="flex items-center gap-2">
      <button
        type="button"
        data-testid="{stepTestidPrefix}-{step.id}"
        data-active={active ? "true" : "false"}
        onclick={() => onSelect?.(step.id)}
        class="flex items-center gap-2 rounded px-2 py-1 text-sm"
        class:font-semibold={active}
        class:text-primary={active}
        class:text-muted-foreground={!active}
      >
        <span
          class="flex h-6 w-6 items-center justify-center rounded-full border text-xs"
          class:border-primary={active}
          class:bg-primary={active}
          class:text-primary-foreground={active}
          class:border-border={!active}
        >
          {i + 1}
        </span>
        <span>{step.label}</span>
      </button>
      {#if i < steps.length - 1}
        <span aria-hidden="true" class="h-px w-6 bg-border"></span>
      {/if}
    </li>
  {/each}
</ol>
