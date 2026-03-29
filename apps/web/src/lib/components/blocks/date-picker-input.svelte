<script lang="ts">
  import { cn } from "$lib/utils.js";
  import { Calendar } from "$lib/components/ui/calendar/index.js";
  import * as Popover from "$lib/components/ui/popover/index.js";
  import Button from "$lib/components/ui/button/button.svelte";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import type { DateValue } from "@internationalized/date";
  import { DateFormatter, getLocalTimeZone } from "@internationalized/date";

  interface Props {
    value?: DateValue;
    placeholder?: string;
    class?: string;
    onchange?: (value: DateValue | undefined) => void;
  }

  let {
    value = $bindable(),
    placeholder = "Pick a date",
    class: className,
    onchange,
  }: Props = $props();

  const df = new DateFormatter("en-US", { dateStyle: "long" });

  let open = $state(false);

  let displayText = $derived(
    value ? df.format(value.toDate(getLocalTimeZone())) : placeholder,
  );

  function handleSelect(selected: DateValue | undefined) {
    value = selected;
    onchange?.(selected);
    open = false;
  }
</script>

<Popover.Root bind:open>
  <Popover.Trigger>
    <Button
      variant="outline"
      class={cn(
        "w-[280px] justify-start text-left font-normal",
        !value && "text-muted-foreground",
        className,
      )}
    >
      <CalendarIcon class="mr-2 size-4" />
      {displayText}
    </Button>
  </Popover.Trigger>
  <Popover.Content class="w-auto p-0" align="start">
    <Calendar type="single" {value} onValueChange={handleSelect} />
  </Popover.Content>
</Popover.Root>
