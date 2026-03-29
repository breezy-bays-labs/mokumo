<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import DatePickerInput from "./date-picker-input.svelte";

  const { Story } = defineMeta({
    title: "Blocks/DatePickerInput",
    component: DatePickerInput,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <DatePickerInput />
</Story>

<Story name="Custom Placeholder">
  <DatePickerInput placeholder="Select due date" />
</Story>
